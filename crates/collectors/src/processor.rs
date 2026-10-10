//! CPU packages from `/proc/cpuinfo` and `/sys/devices/system/cpu`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use facet_core::{Availability, Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{list_dir, parse_field, read_field, source, text_spec};

const PROC_SRC: &str = "procfs:/proc/cpuinfo";
const CPU: &str = "sys/devices/system/cpu";

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

type Block = BTreeMap<String, String>;

/// Split `/proc/cpuinfo` into per-processor key/value blocks. Blocks without a `processor` key are ignored.
fn parse_cpuinfo(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut cur = Block::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if cur.contains_key("processor") {
                out.push(std::mem::take(&mut cur));
            } else {
                cur.clear();
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            cur.entry(k.trim().to_owned()).or_insert_with(|| v.trim().to_owned());
        }
    }
    out
}

/// Parse a kernel cpu list such as `0-15,17`.
pub fn parse_cpu_list(s: &str) -> Option<Vec<u32>> {
    let mut v = Vec::new();
    for part in s.trim().split(',').filter(|p| !p.trim().is_empty()) {
        match part.trim().split_once('-') {
            Some((a, b)) => {
                let (a, b): (u32, u32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
                if b < a || b - a > 100_000 {
                    return None;
                }
                v.extend(a..=b);
            }
            None => v.push(part.trim().parse().ok()?),
        }
    }
    if v.is_empty() { None } else { Some(v) }
}

/// "48K", "2M", "36864K", "1G" or plain bytes -> bytes.
fn parse_cache_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()? {
        'K' | 'k' => (&s[..s.len() - 1], 1024),
        'M' | 'm' => (&s[..s.len() - 1], 1024 * 1024),
        'G' | 'g' => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    num.trim().parse::<u64>().ok()?.checked_mul(mult)
}

fn clean_model(name: &str) -> String {
    let mut s = name.replace("(R)", "").replace("(TM)", "").replace("(tm)", "").replace("(r)", "");
    if let Some(i) = s.find(" @ ") {
        s.truncate(i);
    }
    s.split_whitespace().filter(|w| !w.eq_ignore_ascii_case("cpu") && !w.eq_ignore_ascii_case("processor")).collect::<Vec<_>>().join(" ")
}

fn vendor_name(id: &str) -> String {
    match id {
        "GenuineIntel" => "Intel".into(),
        "AuthenticAMD" => "AMD".into(),
        "HygonGenuine" => "Hygon".into(),
        "CentaurHauls" => "Centaur".into(),
        other => other.to_owned(),
    }
}

fn info_field(b: &Block, key: &str) -> Field<String> {
    match b.get(key) {
        Some(v) if !v.is_empty() => Field::available(v.clone(), PROC_SRC),
        _ => Field::not_reported(PROC_SRC),
    }
}

fn info_count(b: &Block, key: &str) -> Field<Value> {
    parse_field(info_field(b, key), |s| s.parse::<u64>().map(Value::Count).map_err(|e| format!("{key}: {e}")))
}

fn cpu_path(n: u32, rel: &str) -> String {
    format!("{CPU}/cpu{n}/{rel}")
}

/// Which logical CPUs are performance vs efficiency cores, with the source used. Hybrid only.
struct Hybrid {
    perf: BTreeSet<u32>,
    eff: BTreeSet<u32>,
    src: String,
}

fn detect_hybrid(root: &Path, cpus: &[u32]) -> Option<Hybrid> {
    let list = |rel: &str| read_field(root, rel).value().and_then(|s| parse_cpu_list(s));
    if let (Some(p), Some(e)) = (list("sys/devices/cpu_core/cpus"), list("sys/devices/cpu_atom/cpus")) {
        return Some(Hybrid { perf: p.into_iter().collect(), eff: e.into_iter().collect(), src: source("sys/devices/cpu_core/cpus") });
    }
    // Asymmetric designs (Arm big.LITTLE and friends) expose relative capacity per CPU.
    let mut cap = BTreeMap::new();
    for &n in cpus {
        let v: u64 = read_field(root, &cpu_path(n, "cpu_capacity")).value()?.parse().ok()?;
        cap.insert(n, v);
    }
    let distinct: BTreeSet<u64> = cap.values().copied().collect();
    if distinct.len() != 2 {
        return None;
    }
    let hi = *distinct.iter().next_back()?;
    let (perf, eff) = cap.iter().fold((BTreeSet::new(), BTreeSet::new()), |(mut p, mut e), (n, c)| {
        if *c == hi { p.insert(*n) } else { e.insert(*n) };
        (p, e)
    });
    Some(Hybrid { perf, eff, src: source(&cpu_path(cpus[0], "cpu_capacity")) })
}

/// Fold a cpufreq kHz file over `cpus`, keeping the CPU whose value wins so the source is a real path.
fn freq(root: &Path, cpus: &[u32], file: &str, better: fn(u64, u64) -> bool) -> Field<Value> {
    let mut best: Option<(u64, u32)> = None;
    let mut first_err: Option<Field<Value>> = None;
    for &n in cpus {
        let rel = cpu_path(n, &format!("cpufreq/{file}"));
        let f = parse_field(read_field(root, &rel), |s| s.parse::<u64>().map(|k| Value::Hertz(k.saturating_mul(1000))).map_err(|e| e.to_string()));
        match f.value() {
            Some(Value::Hertz(h)) => {
                if best.is_none_or(|(b, _)| better(*h, b)) {
                    best = Some((*h, n));
                }
            }
            _ => {
                first_err.get_or_insert(f);
            }
        }
    }
    match (best, first_err) {
        (Some((h, n)), _) => Field::available(Value::Hertz(h), source(&cpu_path(n, &format!("cpufreq/{file}")))),
        (None, Some(e)) => e,
        (None, None) => Field::not_reported(source(&cpu_path(cpus.first().copied().unwrap_or(0), &format!("cpufreq/{file}")))),
    }
}

fn cache_specs(root: &Path, first: u32, pkg_cpus: usize) -> Vec<Spec> {
    let dir = cpu_path(first, "cache");
    let mut found: Vec<(u32, u8, Spec)> = Vec::new();
    for ix in list_dir(root, &dir).into_iter().filter(|n| n.starts_with("index")) {
        let base = format!("{dir}/{ix}");
        let level = read_field(root, &format!("{base}/level")).value().and_then(|s| s.parse::<u32>().ok());
        let kind = read_field(root, &format!("{base}/type")).value().cloned();
        let size = read_field(root, &format!("{base}/size"));
        let (Some(level), Some(kind)) = (level, kind) else { continue };
        let sharing = read_field(root, &format!("{base}/shared_cpu_list")).value().and_then(|s| parse_cpu_list(s)).map(|l| l.len());
        let name = match (level, kind.as_str()) {
            (1, "Data") => "L1 data cache".to_owned(),
            (1, "Instruction") => "L1 instruction cache".to_owned(),
            (l, _) => format!("L{l} cache"),
        };
        let label = match sharing {
            Some(1) => format!("{name} (per thread)"),
            Some(n) if n >= pkg_cpus && pkg_cpus > 1 => format!("{name} (shared)"),
            Some(n) => format!("{name} (per {n} threads)"),
            None => name,
        };
        let order = if kind == "Instruction" { 1 } else { 0 };
        let spec = Spec::new(&label, parse_field(size, |s| parse_cache_size(s).map(Value::BinaryBytes).ok_or_else(|| format!("unrecognised cache size {s:?}"))));
        found.push((level, order, spec));
    }
    found.sort_by_key(|(l, o, _)| (*l, *o));
    found.into_iter().map(|(_, _, s)| s).collect()
}

const NOTABLE_FLAGS: [&str; 24] = [
    "sse4_1", "sse4_2", "avx", "avx2", "fma", "aes", "vaes", "pclmulqdq", "sha_ni", "bmi1", "bmi2", "rdrand", "rdseed", "avx_vnni", "amx_tile", "amx_int8",
    "amx_bf16", "sgx", "sev", "neon", "asimd", "sve", "pmull", "crc32",
];

fn feature_specs(block: &Block) -> Vec<Spec> {
    let key = if block.contains_key("flags") { "flags" } else { "Features" };
    let Some(line) = block.get(key) else {
        return vec![Spec::missing("Instruction set extensions", PROC_SRC), Spec::missing("Hardware virtualization", PROC_SRC)];
    };
    let flags: BTreeSet<&str> = line.split_whitespace().collect();
    let mut present: Vec<String> = NOTABLE_FLAGS.iter().filter(|f| flags.contains(**f)).map(|f| (*f).to_owned()).collect();
    present.extend(flags.iter().filter(|f| f.starts_with("avx512")).map(|f| (*f).to_owned()));
    let virt = if flags.contains("vmx") {
        "Intel VT-x (vmx)"
    } else if flags.contains("svm") {
        "AMD-V (svm)"
    } else {
        "Not advertised by the CPU (may be disabled in firmware)"
    };
    let text = if present.is_empty() { "None of the tracked extensions".to_owned() } else { present.join(", ") };
    vec![Spec::text("Instruction set extensions", text, PROC_SRC), Spec::text("Hardware virtualization", virt, PROC_SRC)]
}

fn vulnerability_group(root: &Path) -> Option<SpecGroup> {
    let dir = format!("{CPU}/vulnerabilities");
    let names = list_dir(root, &dir);
    if names.is_empty() {
        return None;
    }
    let specs = names.iter().map(|n| text_spec(n, read_field(root, &format!("{dir}/{n}")))).collect();
    Some(SpecGroup::new("Vulnerability mitigations", specs).technical())
}

fn count_spec(label: &str, n: usize, src: &str) -> Spec {
    Spec::val(label, Value::Count(n as u64), src)
}

fn distinct_cores(procs: &[(u32, &Block)], filter: Option<&BTreeSet<u32>>) -> Option<usize> {
    let mut set = BTreeSet::new();
    for (n, b) in procs {
        if filter.is_some_and(|f| !f.contains(n)) {
            continue;
        }
        set.insert(b.get("core id")?.clone());
    }
    Some(set.len())
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let info = read_field(root, "proc/cpuinfo");
    let text = match info.availability {
        Availability::Available { value } => value,
        Availability::NotReported => return Vec::new(),
        other => {
            let f = Field { availability: other, source: PROC_SRC.into() };
            let spec = Spec::new("Processor details", parse_field(f, |s| Ok(Value::Text(s.clone()))));
            return vec![Component::new("cpu:package0", Category::Processor, "Processor").summary(vec![spec])];
        }
    };
    let blocks = parse_cpuinfo(&text);
    if blocks.is_empty() {
        let spec = Spec::new("Processor details", Field::failed("no processor entries found in /proc/cpuinfo", PROC_SRC));
        return vec![Component::new("cpu:package0", Category::Processor, "Processor").summary(vec![spec])];
    }

    // Group logical processors by package; processors without a number are ignored (malformed).
    let mut packages: BTreeMap<String, Vec<(u32, &Block)>> = BTreeMap::new();
    for b in &blocks {
        let Some(n) = b.get("processor").and_then(|s| s.parse::<u32>().ok()) else { continue };
        packages.entry(b.get("physical id").cloned().unwrap_or_else(|| "0".into())).or_default().push((n, b));
    }
    if packages.is_empty() {
        let spec = Spec::new("Processor details", Field::failed("processor numbers in /proc/cpuinfo are not numeric", PROC_SRC));
        return vec![Component::new("cpu:package0", Category::Processor, "Processor").summary(vec![spec])];
    }
    let sockets = packages.len();

    let mut ordered: Vec<(String, Vec<(u32, &Block)>)> = packages.into_iter().collect();
    ordered.sort_by_key(|(k, _)| k.parse::<u32>().unwrap_or(u32::MAX));
    ordered.into_iter().enumerate().map(|(i, (phys, mut procs))| {
        procs.sort_by_key(|(n, _)| *n);
        let id = format!("cpu:package{}", phys.parse::<u32>().unwrap_or(i as u32));
        package_component(root, &id, &procs, sockets)
    }).collect()
}

fn package_component(root: &Path, id: &str, procs: &[(u32, &Block)], sockets: usize) -> Component {
    let first = procs[0].1;
    let cpus: Vec<u32> = procs.iter().map(|(n, _)| *n).collect();
    let threads = procs.len();
    let cores = distinct_cores(procs, None);
    let hybrid = detect_hybrid(root, &cpus);

    let model_name = first.get("model name").or_else(|| first.get("Model Name")).or_else(|| first.get("Hardware")).cloned();
    let title = model_name.as_deref().map(clean_model).filter(|s| !s.is_empty()).unwrap_or_else(|| "Processor".to_owned());

    let max_f = freq(root, &cpus, "cpuinfo_max_freq", |a, b| a > b);
    let min_f = freq(root, &cpus, "cpuinfo_min_freq", |a, b| a < b);
    let cur_f = freq(root, &cpus, "scaling_cur_freq", |a, b| a > b);

    let cores_spec = match cores {
        Some(n) => count_spec("Cores", n, PROC_SRC),
        None => Spec::missing("Cores", PROC_SRC),
    };
    let threads_spec = count_spec("Threads", threads, PROC_SRC);
    let max_spec = Spec::new("Maximum frequency", max_f.clone()).rated();

    let summary = vec![cores_spec.clone(), threads_spec.clone(), max_spec];

    let mut core_specs = vec![cores_spec, threads_spec];
    if let Some(h) = &hybrid {
        let p: BTreeSet<u32> = h.perf.iter().copied().filter(|n| cpus.contains(n)).collect();
        let e: BTreeSet<u32> = h.eff.iter().copied().filter(|n| cpus.contains(n)).collect();
        core_specs.push(Spec::val("Hybrid design", Value::Bool(true), &h.src));
        match distinct_cores(procs, Some(&p)) {
            Some(n) => core_specs.push(count_spec("Performance cores", n, &h.src)),
            None => core_specs.push(Spec::missing("Performance cores", PROC_SRC)),
        }
        core_specs.push(count_spec("Performance-core threads", p.len(), &h.src));
        match distinct_cores(procs, Some(&e)) {
            Some(n) => core_specs.push(count_spec("Efficiency cores", n, &h.src)),
            None => core_specs.push(Spec::missing("Efficiency cores", PROC_SRC)),
        }
        core_specs.push(count_spec("Efficiency-core threads", e.len(), &h.src));
    }

    let arch = {
        let flags = first.get("flags");
        match (flags, first.get("CPU architecture")) {
            (Some(f), _) if f.split_whitespace().any(|w| w == "lm") => Spec::text("Architecture", "x86-64", PROC_SRC),
            (Some(_), _) => Spec::text("Architecture", "x86 (32-bit)", PROC_SRC),
            (None, Some(a)) => Spec::text("Architecture", format!("Arm (architecture {a})"), PROC_SRC),
            _ => Spec::missing("Architecture", PROC_SRC),
        }
    };
    let vendor = first.get("vendor_id").map(|v| vendor_name(v));
    let vendor_spec = match vendor {
        Some(v) => Spec::text("Vendor", v, PROC_SRC),
        None => Spec::missing("Vendor", PROC_SRC),
    };

    let mut cache = cache_specs(root, cpus[0], threads);
    if cache.is_empty() {
        cache.push(match first.get("cache size") {
            Some(s) => Spec::new(
                "Cache size (as reported by CPU)",
                parse_field(Field::available(s.clone(), PROC_SRC), |s| {
                    let kb = s.trim_end_matches("KB").trim().parse::<u64>().map_err(|_| format!("unrecognised cache size {s:?}"))?;
                    Ok(Value::BinaryBytes(kb * 1024))
                }),
            ),
            None => Spec::missing("Cache", &source(&cpu_path(cpus[0], "cache"))),
        });
    }

    let microcode = match first.get("microcode") {
        Some(m) => Spec::text("Microcode", m.clone(), PROC_SRC),
        None => text_spec("Microcode", read_field(root, &cpu_path(cpus[0], "microcode/version"))),
    };

    let mut freq_specs = vec![
        Spec::new("Minimum frequency", min_f).rated(),
        Spec::new("Maximum frequency", max_f).rated(),
        Spec::new("Current frequency (fastest core)", cur_f).live(),
    ];
    freq_specs.push(text_spec("Scaling driver", read_field(root, &cpu_path(cpus[0], "cpufreq/scaling_driver"))));
    freq_specs.push(text_spec("Governor", read_field(root, &cpu_path(cpus[0], "cpufreq/scaling_governor"))).live());

    let mut c = Component::new(id, Category::Processor, &title).summary(summary);
    c.subtitle = match cores {
        Some(n) => Some(format!("{n} cores, {threads} threads")),
        None => Some(format!("{threads} threads")),
    };
    c = c
        .group(SpecGroup::new("Overview", vec![
            vendor_spec,
            model_name.map_or_else(|| Spec::missing("Model name", PROC_SRC), |m| Spec::text("Model name", m, PROC_SRC)),
            arch,
            count_spec("Sockets", sockets, PROC_SRC),
        ]))
        .group(SpecGroup::new("Cores", core_specs))
        .group(SpecGroup::new("Frequency", freq_specs))
        .group(SpecGroup::new("Caches (as seen from the first logical CPU)", cache))
        .group(
            SpecGroup::new("Identification", vec![
                Spec::new("Family", info_count(first, "cpu family")),
                Spec::new("Model", info_count(first, "model")),
                Spec::new("Stepping", info_count(first, "stepping")),
                microcode,
            ])
            .technical(),
        )
        .group(SpecGroup::new("Instruction set features", feature_specs(first)).technical());
    if let Some(g) = vulnerability_group(root) {
        c = c.group(g);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    const FLAGS: &str = "fpu vme lm sse4_1 sse4_2 avx avx2 aes vmx avx_vnni sha_ni";

    fn block(n: u32, phys: u32, core: u32, flags: &str) -> String {
        format!(
            "processor\t: {n}\nvendor_id\t: GenuineIntel\ncpu family\t: 6\nmodel\t\t: 183\nmodel name\t: Intel(R) Core(TM) i9-14900HX\nstepping\t: 1\nmicrocode\t: 0x137\ncpu MHz\t\t: 846.965\ncache size\t: 36864 KB\nphysical id\t: {phys}\nsiblings\t: 32\ncore id\t\t: {core}\ncpu cores\t: 24\nflags\t\t: {flags}\n\n"
        )
    }

    fn i9() -> Fx {
        let mut info = String::new();
        for k in 0..8u32 {
            info += &block(2 * k, 0, 4 * k, FLAGS);
            info += &block(2 * k + 1, 0, 4 * k, FLAGS);
        }
        for j in 0..16u32 {
            info += &block(16 + j, 0, 32 + j, FLAGS);
        }
        Fx::new(&[
            ("proc/cpuinfo", &info),
            ("sys/devices/cpu_core/cpus", "0-15\n"),
            ("sys/devices/cpu_atom/cpus", "16-31\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/level", "1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/type", "Data\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/size", "48K\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/shared_cpu_list", "0-1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index1/level", "1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index1/type", "Instruction\n"),
            ("sys/devices/system/cpu/cpu0/cache/index1/size", "32K\n"),
            ("sys/devices/system/cpu/cpu0/cache/index1/shared_cpu_list", "0-1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index2/level", "2\n"),
            ("sys/devices/system/cpu/cpu0/cache/index2/type", "Unified\n"),
            ("sys/devices/system/cpu/cpu0/cache/index2/size", "2048K\n"),
            ("sys/devices/system/cpu/cpu0/cache/index2/shared_cpu_list", "0-1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index3/level", "3\n"),
            ("sys/devices/system/cpu/cpu0/cache/index3/type", "Unified\n"),
            ("sys/devices/system/cpu/cpu0/cache/index3/size", "36864K\n"),
            ("sys/devices/system/cpu/cpu0/cache/index3/shared_cpu_list", "0-31\n"),
            ("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq", "5800000\n"),
            ("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq", "800000\n"),
            ("sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq", "846965\n"),
            ("sys/devices/system/cpu/cpu0/cpufreq/scaling_driver", "intel_pstate\n"),
            ("sys/devices/system/cpu/cpu16/cpufreq/cpuinfo_max_freq", "4100000\n"),
            ("sys/devices/system/cpu/cpu16/cpufreq/cpuinfo_min_freq", "800000\n"),
            ("sys/devices/system/cpu/cpu16/cpufreq/scaling_cur_freq", "1200000\n"),
            ("sys/devices/system/cpu/vulnerabilities/spectre_v2", "Mitigation: Enhanced / Automatic IBRS\n"),
            ("sys/devices/system/cpu/vulnerabilities/meltdown", "Not affected\n"),
        ])
    }

    #[test]
    fn real_machine_hybrid_i9() {
        let fx = i9();
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "cpu:package0");
        assert_eq!(c.category, Category::Processor);
        assert_eq!(c.title, "Intel Core i9-14900HX");
        assert_eq!(c.subtitle.as_deref(), Some("24 cores, 32 threads"));
        assert_eq!(spec(c, "Cores").value.value(), Some(&Value::Count(24)));
        assert_eq!(spec(c, "Threads").value.value(), Some(&Value::Count(32)));
        assert_eq!(spec(c, "Performance cores").value.value(), Some(&Value::Count(8)));
        assert_eq!(spec(c, "Performance-core threads").value.value(), Some(&Value::Count(16)));
        assert_eq!(spec(c, "Efficiency cores").value.value(), Some(&Value::Count(16)));
        assert_eq!(spec(c, "Efficiency-core threads").value.value(), Some(&Value::Count(16)));
        assert_eq!(spec(c, "Sockets").value.value(), Some(&Value::Count(1)));
        let max = c.summary.iter().find(|s| s.label == "Maximum frequency").unwrap();
        assert_eq!(max.display(), "5.80 GHz");
        assert!(max.rated);
        assert_eq!(max.value.source, "sysfs:/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq");
        assert_eq!(spec(c, "Minimum frequency").display(), "800 MHz");
        let cur = spec(c, "Current frequency (fastest core)");
        assert!(cur.live);
        assert_eq!(cur.display(), "1.20 GHz");
        assert_eq!(cur.value.source, "sysfs:/sys/devices/system/cpu/cpu16/cpufreq/scaling_cur_freq");
        assert_eq!(spec(c, "L1 data cache (per 2 threads)").value.value(), Some(&Value::BinaryBytes(48 * 1024)));
        assert_eq!(spec(c, "L2 cache (per 2 threads)").value.value(), Some(&Value::BinaryBytes(2 * 1024 * 1024)));
        assert_eq!(spec(c, "L3 cache (shared)").value.value(), Some(&Value::BinaryBytes(36864 * 1024)));
        assert_eq!(spec(c, "Vendor").display(), "Intel");
        assert_eq!(spec(c, "Architecture").display(), "x86-64");
        assert_eq!(spec(c, "Family").display(), "6");
        assert_eq!(spec(c, "Model").display(), "183");
        assert_eq!(spec(c, "Microcode").display(), "0x137");
        assert_eq!(spec(c, "Hardware virtualization").display(), "Intel VT-x (vmx)");
        assert!(spec(c, "Instruction set extensions").display().contains("avx2"));
        assert_eq!(spec(c, "meltdown").display(), "Not affected");
        assert!(c.groups.iter().any(|g| g.technical && g.title == "Vulnerability mitigations"));
    }

    #[test]
    fn two_sockets_yield_two_packages() {
        let mut info = String::new();
        for s in 0..2u32 {
            for k in 0..4u32 {
                info += &block(s * 4 + k, s, k, FLAGS);
            }
        }
        let fx = Fx::new(&[("proc/cpuinfo", &info)]);
        let v = collect_from(fx.path());
        assert_eq!(v.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["cpu:package0", "cpu:package1"]);
        for c in &v {
            assert_eq!(spec(c, "Sockets").value.value(), Some(&Value::Count(2)));
            assert_eq!(spec(c, "Cores").value.value(), Some(&Value::Count(4)));
            assert_eq!(spec(c, "Threads").value.value(), Some(&Value::Count(4)));
        }
    }

    #[test]
    fn vm_guest_without_cpufreq_cache_or_sysfs() {
        let flags = "fpu lm hypervisor";
        let info = (0..4).map(|n| block(n, 0, n, flags)).collect::<String>();
        let fx = Fx::new(&[("proc/cpuinfo", &info)]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Cores").display(), "4");
        for l in ["Maximum frequency", "Minimum frequency", "Current frequency (fastest core)"] {
            assert_eq!(spec(c, l).display(), "Not reported", "{l}");
        }
        assert!(c.groups.iter().all(|g| g.title != "Vulnerability mitigations"));
        assert!(c.all_specs().all(|s| s.label != "Hybrid design"));
        assert!(spec(c, "Cache size (as reported by CPU)").is_available());
    }

    #[test]
    fn empty_cache_dir_falls_back_to_cpuinfo_size() {
        let fx = Fx::new(&[("proc/cpuinfo", &block(0, 0, 0, FLAGS)), ("sys/devices/system/cpu/cpu0/cache/.keep", "")]);
        let c = &collect_from(fx.path())[0];
        let s = spec(c, "Cache size (as reported by CPU)");
        assert_eq!(s.value.value(), Some(&Value::BinaryBytes(36864 * 1024)));
        assert_eq!(s.value.source, "procfs:/proc/cpuinfo");
    }

    #[test]
    fn capacity_based_hybrid_detection() {
        let info = (0..4).map(|n| block(n, 0, n, FLAGS)).collect::<String>();
        let fx = Fx::new(&[
            ("proc/cpuinfo", &info),
            ("sys/devices/system/cpu/cpu0/cpu_capacity", "1024\n"),
            ("sys/devices/system/cpu/cpu1/cpu_capacity", "1024\n"),
            ("sys/devices/system/cpu/cpu2/cpu_capacity", "512\n"),
            ("sys/devices/system/cpu/cpu3/cpu_capacity", "512\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Performance cores").display(), "2");
        assert_eq!(spec(c, "Efficiency cores").display(), "2");
        assert!(spec(c, "Hybrid design").value.source.ends_with("cpu_capacity"));
    }

    #[test]
    fn symmetric_capacity_is_not_hybrid() {
        let info = (0..2).map(|n| block(n, 0, n, FLAGS)).collect::<String>();
        let fx = Fx::new(&[
            ("proc/cpuinfo", &info),
            ("sys/devices/system/cpu/cpu0/cpu_capacity", "1024\n"),
            ("sys/devices/system/cpu/cpu1/cpu_capacity", "1024\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert!(c.all_specs().all(|s| s.label != "Hybrid design"));
    }

    #[test]
    fn malformed_values_fail_individually() {
        let info = "processor\t: 0\nvendor_id\t: AuthenticAMD\ncpu family\t: banana\nmodel name\t: AMD Ryzen 7 7840HS with Radeon 780M Graphics\nflags\t\t: fpu lm svm\ncore id\t: 0\n\n";
        let fx = Fx::new(&[
            ("proc/cpuinfo", info),
            ("sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq", "fast\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/level", "1\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/type", "Data\n"),
            ("sys/devices/system/cpu/cpu0/cache/index0/size", "lots\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Family").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Maximum frequency").value.availability, Availability::Failed { .. }));
        assert!(c.all_specs().any(|s| s.label.starts_with("L1 data cache") && matches!(s.value.availability, Availability::Failed { .. })));
        assert_eq!(spec(c, "Vendor").display(), "AMD");
        assert_eq!(spec(c, "Hardware virtualization").display(), "AMD-V (svm)");
        assert_eq!(spec(c, "Stepping").display(), "Not reported");
    }

    #[test]
    fn garbage_cpuinfo_reports_failure_and_missing_reports_nothing() {
        let fx = Fx::new(&[("proc/cpuinfo", "this is not cpuinfo\nat all\n")]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        assert!(matches!(spec(&v[0], "Processor details").value.availability, Availability::Failed { .. }));
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    #[test]
    fn restricted_vulnerability_file_requires_permission() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new(&[("proc/cpuinfo", &block(0, 0, 0, FLAGS)), ("sys/devices/system/cpu/vulnerabilities/mds", "Not affected\n")]);
        let p = fx.path().join("sys/devices/system/cpu/vulnerabilities/mds");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&p).is_ok() {
            return; // running as root
        }
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "mds").value.availability, Availability::PermissionRequired));
    }

    #[test]
    #[ignore]
    fn real_machine_dump() {
        for c in collect() {
            println!("== {} [{}] {} / {:?}", c.id, c.category.label(), c.title, c.subtitle);
            for s in c.all_specs() {
                println!("  {:<40} {}  <{}>{}{}", s.label, s.display(), s.value.source, if s.live { " live" } else { "" }, if s.rated { " rated" } else { "" });
            }
        }
    }

    #[test]
    fn cpu_list_and_size_parsing() {
        assert_eq!(parse_cpu_list("0-3,8"), Some(vec![0, 1, 2, 3, 8]));
        assert_eq!(parse_cpu_list("3-1"), None);
        assert_eq!(parse_cpu_list("x"), None);
        assert_eq!(parse_cpu_list(""), None);
        assert_eq!(parse_cache_size("1M"), Some(1 << 20));
        assert_eq!(parse_cache_size("abc"), None);
        assert_eq!(clean_model("Intel(R) Xeon(R) CPU E5-2680 v4 @ 2.40GHz"), "Intel Xeon E5-2680 v4");
    }
}
