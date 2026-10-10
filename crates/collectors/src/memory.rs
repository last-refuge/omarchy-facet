//! System memory from `/proc/meminfo`, swap from `/proc/swaps` (+ zram sysfs), and, when the
//! SMBIOS table is readable, one component per installed memory module (type 17).

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use facet_core::{Availability, Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{at, parse_field, read_field};

const MEMINFO_SRC: &str = "procfs:/proc/meminfo";
const SWAPS_SRC: &str = "procfs:/proc/swaps";
const DMI_REL: &str = "sys/firmware/dmi/tables/DMI";
const DMI_SRC: &str = "dmi:/sys/firmware/dmi/tables/DMI";
const SYSTEM_ID: &str = "memory:system";

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let meminfo = read_field(root, "proc/meminfo");
    if matches!(meminfo.availability, Availability::NotReported) {
        return Vec::new();
    }

    let dimms = read_dimms(root);
    let mut out = vec![system_component(root, meminfo, &dimms)];
    if let Ok(Some(list)) = &dimms {
        out.extend(list.iter().filter(|d| d.size != Some(0)).map(dimm_component));
    }
    out
}

// ---------------------------------------------------------------- meminfo

fn kib_map(f: &Field<String>) -> Result<BTreeMap<String, Result<u64, String>>, Field<Value>> {
    match &f.availability {
        Availability::Available { value } => Ok(value
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| {
                let n = v.trim().trim_end_matches("kB").trim();
                (k.trim().to_owned(), n.parse::<u64>().map(|kb| kb.saturating_mul(1024)).map_err(|_| format!("{k}: unexpected value {:?}", v.trim())))
            })
            .collect()),
        Availability::PermissionRequired => Err(Field::permission_required(MEMINFO_SRC)),
        Availability::Failed { reason } => Err(Field::failed(reason.clone(), MEMINFO_SRC)),
        _ => Err(Field::not_reported(MEMINFO_SRC)),
    }
}

fn mem_spec(label: &str, map: &Result<BTreeMap<String, Result<u64, String>>, Field<Value>>, key: &str) -> Spec {
    let field = match map {
        Err(f) => f.clone(),
        Ok(m) => match m.get(key) {
            Some(Ok(b)) => Field::available(Value::BinaryBytes(*b), MEMINFO_SRC),
            Some(Err(e)) => Field::failed(e.clone(), MEMINFO_SRC),
            None => Field::not_reported(MEMINFO_SRC),
        },
    };
    Spec::new(label, field)
}

fn system_component(root: &Path, meminfo: Field<String>, dimms: &Result<Option<Vec<Dimm>>, Field<Value>>) -> Component {
    let m = kib_map(&meminfo);
    let total = mem_spec("Usable memory", &m, "MemTotal");
    let available = mem_spec("Available memory", &m, "MemAvailable").live();
    let swap_total = mem_spec("Swap total", &m, "SwapTotal");

    let title = match total.value.value() {
        Some(Value::BinaryBytes(b)) => format!("{} memory", gib_label(*b)),
        _ => "System memory".to_owned(),
    };

    let usage = SpecGroup::new("Current usage", vec![
        mem_spec("Free", &m, "MemFree").live(),
        mem_spec("Cached", &m, "Cached").live(),
        mem_spec("Buffers", &m, "Buffers").live(),
        mem_spec("Swap free", &m, "SwapFree").live(),
    ]);

    let modules = match dimms {
        Ok(Some(list)) => vec![
            Spec::val("Memory modules", Value::Count(list.iter().filter(|d| d.size != Some(0)).count() as u64), DMI_SRC),
            Spec::val("Memory slots", Value::Count(list.len() as u64), DMI_SRC),
        ],
        Ok(None) => vec![Spec::missing("Memory modules", DMI_SRC)],
        Err(f) => vec![Spec::new("Memory modules", f.clone())],
    };
    let modules_title = match dimms {
        Err(f) if matches!(f.availability, Availability::PermissionRequired) => "Memory modules (SMBIOS; needs elevated access)",
        _ => "Memory modules (SMBIOS)",
    };

    let mut c = Component::new(SYSTEM_ID, Category::Memory, &title)
        .summary(vec![total, available, swap_total])
        .group(usage)
        .group(SpecGroup::new(modules_title, modules));
    c.subtitle = Some("Usable by the operating system; installed capacity comes from the module table".into());
    for g in swap_groups(root) {
        c = c.group(g);
    }
    c
}

/// Binary (GiB) label for human titles. `Value::Bytes` formats in decimal units.
fn gib_label(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        let g = b / GIB;
        // Kernel totals are a little under the installed size; round to a whole GiB when close.
        if (g - g.round()).abs() < 0.05 * g.round().max(1.0) { format!("{} GiB", g.round() as u64) } else { format!("{g:.1} GiB") }
    } else {
        format!("{} MiB", (b / MIB).round() as u64)
    }
}

// ------------------------------------------------------------------- swap

fn swap_groups(root: &Path) -> Vec<SpecGroup> {
    let f = read_field(root, "proc/swaps");
    let text = match f.availability {
        Availability::Available { value } => value,
        Availability::PermissionRequired => return vec![SpecGroup::new("Swap", vec![Spec::new("Swap devices", Field::permission_required(SWAPS_SRC))])],
        Availability::Failed { reason } => return vec![SpecGroup::new("Swap", vec![Spec::new("Swap devices", Field::failed(reason, SWAPS_SRC))])],
        _ => return Vec::new(),
    };
    let mut groups = Vec::new();
    for (i, line) in text.lines().enumerate().skip(1).filter(|(_, l)| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (Some(&name), Some(&kind), Some(size), Some(used)) =
            (cols.first(), cols.get(1), cols.get(2).and_then(|s| s.parse::<u64>().ok()), cols.get(3).and_then(|s| s.parse::<u64>().ok()))
        else {
            groups.push(SpecGroup::new(&format!("Swap entry on line {}", i + 1), vec![Spec::new("Swap device", Field::failed(format!("malformed line {line:?}"), SWAPS_SRC))]));
            continue;
        };
        let path = name.replace("\\040", " ");
        let base = path.rsplit('/').next().unwrap_or("");
        let zram = base.starts_with("zram");
        let kind_label = if zram {
            "Compressed RAM (zram)".to_owned()
        } else {
            match kind {
                "file" => "File".to_owned(),
                "partition" => "Partition".to_owned(),
                other => other.to_owned(),
            }
        };
        let mut specs = vec![
            Spec::text("Kind", kind_label, SWAPS_SRC),
            Spec::val("Size", Value::BinaryBytes(size.saturating_mul(1024)), SWAPS_SRC),
            Spec::val("Used", Value::BinaryBytes(used.saturating_mul(1024)), SWAPS_SRC).live(),
            match cols.get(4) {
                Some(p) => Spec::text("Priority", *p, SWAPS_SRC),
                None => Spec::missing("Priority", SWAPS_SRC),
            },
        ];
        if zram {
            specs.extend(zram_specs(root, base));
        }
        groups.push(SpecGroup::new(&format!("Swap: {path}"), specs));
    }
    groups
}

fn zram_specs(root: &Path, name: &str) -> Vec<Spec> {
    let dir = format!("sys/block/{name}");
    let algo = parse_field(read_field(root, &format!("{dir}/comp_algorithm")), |s| {
        s.split_whitespace()
            .find(|t| t.starts_with('[') && t.ends_with(']'))
            .map(|t| Value::Text(t.trim_matches(|c| c == '[' || c == ']').to_owned()))
            .ok_or_else(|| format!("no active algorithm in {s:?}"))
    });
    let disksize = parse_field(read_field(root, &format!("{dir}/disksize")), |s| s.parse::<u64>().map(Value::BinaryBytes).map_err(|e| e.to_string()));
    let mm_path = format!("{dir}/mm_stat");
    let mm = |idx: usize| {
        parse_field(read_field(root, &mm_path), |s| {
            s.split_whitespace().nth(idx).ok_or_else(|| "mm_stat is shorter than expected".to_owned())?.parse::<u64>().map(Value::BinaryBytes).map_err(|e| e.to_string())
        })
    };
    vec![
        Spec::new("Compression algorithm", algo),
        Spec::new("Uncompressed capacity", disksize),
        Spec::new("Data stored (uncompressed)", mm(0)).live(),
        Spec::new("Data stored (compressed)", mm(1)).live(),
    ]
}

// ------------------------------------------------------------ SMBIOS type 17

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dimm {
    /// Position among type-17 structures, so ids stay stable when slots are empty.
    pub slot: usize,
    /// `Some(0)` = empty slot, `None` = installed but size unknown.
    pub size: Option<u64>,
    pub locator: Option<String>,
    pub bank: Option<String>,
    pub form_factor: Option<&'static str>,
    pub mem_type: Option<String>,
    /// Rated speed, MT/s.
    pub speed: Option<u32>,
    /// Configured (running) speed, MT/s.
    pub configured_speed: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub serial: Option<String>,
}

fn form_factor(code: u8) -> Option<&'static str> {
    Some(match code {
        1 => "Other",
        3 => "SIMM",
        4 => "SIP",
        5 => "Chip",
        6 => "DIP",
        7 => "ZIP",
        8 => "Proprietary card",
        9 => "DIMM",
        0xA => "TSOP",
        0xB => "Row of chips",
        0xC => "RIMM",
        0xD => "SO-DIMM",
        0xE => "SRIMM",
        0xF => "FB-DIMM",
        0x10 => "Die",
        _ => return None,
    })
}

fn memory_type(code: u8) -> Option<String> {
    Some(
        match code {
            0x12 => "DDR",
            0x13 => "DDR2",
            0x14 => "DDR2 FB-DIMM",
            0x18 => "DDR3",
            0x1A => "DDR4",
            0x1B => "LPDDR",
            0x1C => "LPDDR2",
            0x1D => "LPDDR3",
            0x1E => "LPDDR4",
            0x1F => "Logical non-volatile device",
            0x20 => "HBM",
            0x21 => "HBM2",
            0x22 => "DDR5",
            0x23 => "LPDDR5",
            0x24 => "HBM3",
            0x01 | 0x02 => return None,
            other => return Some(format!("Type 0x{other:02X}")),
        }
        .to_owned(),
    )
}

fn smbios_string(strings: &[&[u8]], idx: u8) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let s = String::from_utf8_lossy(strings.get(idx as usize - 1)?).trim().to_owned();
    let l = s.to_lowercase();
    if s.is_empty() || ["not specified", "unknown", "to be filled by o.e.m.", "[empty]", "none", "default string"].contains(&l.as_str()) { None } else { Some(s) }
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

fn parse_type17(slot: usize, d: &[u8], strings: &[&[u8]]) -> Dimm {
    let size = u16_at(d, 0x0C).and_then(|raw| match raw {
        0 => Some(0),
        0xFFFF => None,
        0x7FFF => u32_at(d, 0x1C).map(|mb| u64::from(mb & 0x7FFF_FFFF) << 20),
        r if r & 0x8000 != 0 => Some(u64::from(r & 0x7FFF) << 10),
        r => Some(u64::from(r) << 20),
    });
    let speed = |base: usize, ext: usize| match u16_at(d, base) {
        Some(0) | None => None,
        Some(0xFFFF) => u32_at(d, ext).filter(|v| *v != 0),
        Some(v) => Some(u32::from(v)),
    };
    let string = |o: usize| d.get(o).and_then(|i| smbios_string(strings, *i));
    Dimm {
        slot,
        size,
        locator: string(0x10),
        bank: string(0x11),
        form_factor: d.get(0x0E).and_then(|c| form_factor(*c)),
        mem_type: d.get(0x12).and_then(|c| memory_type(*c)),
        speed: speed(0x15, 0x54),
        configured_speed: speed(0x20, 0x58),
        manufacturer: string(0x17),
        serial: string(0x18),
        part_number: string(0x1A),
    }
}

/// Walk an SMBIOS structure table and decode every type-17 entry. Truncated tables yield what was complete.
pub fn parse_smbios_memory(data: &[u8]) -> Result<Vec<Dimm>, String> {
    let (mut pos, mut seen, mut slot) = (0usize, 0usize, 0usize);
    let mut out = Vec::new();
    while pos + 4 <= data.len() {
        let (ty, len) = (data[pos], data[pos + 1] as usize);
        if len < 4 || pos + len > data.len() {
            break;
        }
        let s_start = pos + len;
        let Some(end) = (s_start..data.len().saturating_sub(1)).find(|&i| data[i] == 0 && data[i + 1] == 0) else { break };
        seen += 1;
        if ty == 127 {
            break;
        }
        if ty == 17 {
            let strings: Vec<&[u8]> = data[s_start..end].split(|b| *b == 0).collect();
            out.push(parse_type17(slot, &data[pos..pos + len], &strings));
            slot += 1;
        }
        pos = end + 2;
    }
    if seen == 0 { Err("not a valid SMBIOS structure table".into()) } else { Ok(out) }
}

/// Ok(Some) = parsed, Ok(None) = table not present, Err = field explaining why not (EACCES, malformed).
fn read_dimms(root: &Path) -> Result<Option<Vec<Dimm>>, Field<Value>> {
    match fs::read(at(root, DMI_REL)) {
        Ok(bytes) => match parse_smbios_memory(&bytes) {
            Ok(v) if v.is_empty() => Ok(None),
            Ok(v) => Ok(Some(v)),
            Err(e) => Err(Field::failed(e, DMI_SRC)),
        },
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Ok(None),
            ErrorKind::PermissionDenied => Err(Field::permission_required(DMI_SRC)),
            _ => Err(Field::failed(e.to_string(), DMI_SRC)),
        },
    }
}

fn dimm_component(d: &Dimm) -> Component {
    let title = d.part_number.clone().or_else(|| d.locator.clone()).unwrap_or_else(|| format!("Memory module {}", d.slot));
    let mut sub = Vec::new();
    if let Some(s) = d.size.filter(|s| *s > 0) {
        sub.push(gib_label(s));
    }
    if let Some(t) = &d.mem_type {
        sub.push(t.clone());
    }
    if let Some(s) = d.configured_speed.or(d.speed) {
        sub.push(format!("{s} MT/s"));
    }
    let opt_text = |label: &str, v: &Option<String>| match v {
        Some(s) => Spec::text(label, s.clone(), DMI_SRC),
        None => Spec::missing(label, DMI_SRC),
    };
    let speed = |label: &str, v: Option<u32>| match v {
        Some(s) => Spec::text(label, format!("{s} MT/s"), DMI_SRC),
        None => Spec::missing(label, DMI_SRC),
    };
    let size = match d.size {
        Some(b) => Spec::val("Capacity", Value::BinaryBytes(b), DMI_SRC),
        None => Spec::new("Capacity", Field::failed("size reported as unknown by firmware", DMI_SRC)),
    };
    let mut c = Component::new(&format!("memory:dimm{}", d.slot), Category::Memory, &title).parent(SYSTEM_ID).summary(vec![
        size,
        opt_text("Type", &d.mem_type),
        speed("Configured speed", d.configured_speed),
    ]);
    c.subtitle = if sub.is_empty() { None } else { Some(sub.join(" · ")) };
    c.group(SpecGroup::new("Module", vec![
        opt_text("Manufacturer", &d.manufacturer),
        opt_text("Part number", &d.part_number),
        speed("Rated speed", d.speed).rated(),
        speed("Configured speed", d.configured_speed),
        match d.form_factor {
            Some(f) => Spec::text("Form factor", f, DMI_SRC),
            None => Spec::missing("Form factor", DMI_SRC),
        },
        opt_text("Slot", &d.locator),
        opt_text("Bank", &d.bank),
        opt_text("Serial number", &d.serial).identifier(),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    const MEMINFO: &str = "MemTotal:       32567160 kB\nMemFree:         5235304 kB\nMemAvailable:   23046204 kB\nBuffers:            2352 kB\nCached:         14008504 kB\nSwapTotal:      65134200 kB\nSwapFree:       65076348 kB\n";
    const SWAPS: &str = "Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n/swap/swapfile                          file\t\t32567932\t0\t\t0\n/dev/zram0                              partition\t32566268\t57892\t\t100\n";

    fn t17(size: u16, form: u8, ty: u8, speed: u16, conf: u16, strs: &[&str]) -> Vec<u8> {
        let mut d = vec![0u8; 0x22];
        d[0] = 17;
        d[1] = 0x22;
        d[0x0C..0x0E].copy_from_slice(&size.to_le_bytes());
        d[0x0E] = form;
        d[0x10] = 1;
        d[0x11] = 2;
        d[0x12] = ty;
        d[0x15..0x17].copy_from_slice(&speed.to_le_bytes());
        d[0x17] = 3;
        d[0x18] = 4;
        d[0x1A] = 5;
        d[0x20..0x22].copy_from_slice(&conf.to_le_bytes());
        for s in strs {
            d.extend_from_slice(s.as_bytes());
            d.push(0);
        }
        if strs.is_empty() {
            d.push(0);
        }
        d.push(0);
        d
    }

    fn table() -> Vec<u8> {
        let mut t = vec![0, 4, 0, 0, b'x', 0, 0]; // type 0 with one string
        t.extend(t17(16384, 0xD, 0x22, 5600, 5600, &["Controller0-ChannelA", "BANK 0", "Samsung", "SER123", "M425R2GA3BB0-CWMOL"]));
        t.extend(t17(0, 0xD, 0x02, 0, 0, &[]));
        t.extend(t17(0x8000 | 512, 9, 0x1A, 3200, 2933, &["DIMM_B1", "BANK 1", "Not Specified", "Unknown", "P1"]));
        t.extend([127, 4, 0, 0, 0, 0]);
        t
    }

    #[test]
    fn real_machine_meminfo_swap_and_zram() {
        let fx = Fx::new(&[
            ("proc/meminfo", MEMINFO),
            ("proc/swaps", SWAPS),
            ("sys/block/zram0/comp_algorithm", "lzo-rle lzo lz4 lz4hc [zstd] deflate 842 \n"),
            ("sys/block/zram0/disksize", "33347862528\n"),
            ("sys/block/zram0/mm_stat", "46161920 11353664 17207296        0 19054592     1817        0       53       61\n"),
            ("sys/firmware/dmi/tables/x", ""),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "memory:system");
        assert_eq!(c.category, Category::Memory);
        assert_eq!(c.title, "31 GiB memory");
        assert_eq!(spec(c, "Usable memory").value.value(), Some(&Value::BinaryBytes(32567160 * 1024)));
        let avail = spec(c, "Available memory");
        assert!(avail.live);
        assert_eq!(avail.value.value(), Some(&Value::BinaryBytes(23046204 * 1024)));
        assert_eq!(avail.value.source, "procfs:/proc/meminfo");
        assert_eq!(spec(c, "Swap total").value.value(), Some(&Value::BinaryBytes(65134200 * 1024)));
        assert!(c.groups.iter().any(|g| g.title == "Swap: /swap/swapfile"));
        let z = c.groups.iter().find(|g| g.title == "Swap: /dev/zram0").unwrap();
        let get = |l: &str| z.specs.iter().find(|s| s.label == l).unwrap();
        assert_eq!(get("Kind").display(), "Compressed RAM (zram)");
        assert_eq!(get("Compression algorithm").display(), "zstd");
        assert_eq!(get("Uncompressed capacity").value.value(), Some(&Value::BinaryBytes(33347862528)));
        assert_eq!(get("Data stored (compressed)").value.value(), Some(&Value::BinaryBytes(11353664)));
        assert!(get("Used").live);
        assert_eq!(get("Priority").display(), "100");
        let f = c.groups.iter().find(|g| g.title == "Swap: /swap/swapfile").unwrap();
        assert_eq!(f.specs[0].display(), "File");
        assert_eq!(f.specs[1].value.value(), Some(&Value::BinaryBytes(32567932 * 1024)));
    }

    #[test]
    fn dimm_table_missing_says_not_reported() {
        let fx = Fx::new(&[("proc/meminfo", MEMINFO)]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Memory modules").display(), "Not reported");
        assert!(c.groups.iter().all(|g| !g.title.starts_with("Swap")));
    }

    #[test]
    fn dimm_table_restricted_requires_permission() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new(&[("proc/meminfo", MEMINFO)]);
        fx.write_bytes(DMI_REL, &table());
        let p = fx.path().join(DMI_REL);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&p).is_ok() {
            return; // running as root
        }
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let s = spec(&v[0], "Memory modules");
        assert!(matches!(s.value.availability, Availability::PermissionRequired));
        assert_eq!(s.value.source, "dmi:/sys/firmware/dmi/tables/DMI");
        assert!(v[0].groups.iter().any(|g| g.title.contains("elevated access")));
    }

    #[test]
    fn readable_smbios_emits_one_component_per_installed_module() {
        let fx = Fx::new(&[("proc/meminfo", MEMINFO)]);
        fx.write_bytes(DMI_REL, &table());
        let v = collect_from(fx.path());
        assert_eq!(v.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["memory:system", "memory:dimm0", "memory:dimm2"]);
        assert_eq!(spec(&v[0], "Memory modules").display(), "2");
        assert_eq!(spec(&v[0], "Memory slots").display(), "3");
        let d0 = &v[1];
        assert_eq!(d0.parent.as_deref(), Some("memory:system"));
        assert_eq!(d0.title, "M425R2GA3BB0-CWMOL");
        assert_eq!(d0.subtitle.as_deref(), Some("16 GiB · DDR5 · 5600 MT/s"));
        assert_eq!(spec(d0, "Capacity").value.value(), Some(&Value::BinaryBytes(16 << 30)));
        assert_eq!(spec(d0, "Form factor").display(), "SO-DIMM");
        assert_eq!(spec(d0, "Manufacturer").display(), "Samsung");
        assert!(spec(d0, "Rated speed").rated);
        assert!(spec(d0, "Serial number").identifier);
        assert_eq!(spec(d0, "Slot").display(), "Controller0-ChannelA");
        let d2 = &v[2];
        assert_eq!(spec(d2, "Capacity").value.value(), Some(&Value::BinaryBytes(512 << 10)));
        assert_eq!(spec(d2, "Manufacturer").display(), "Not reported");
        assert_eq!(spec(d2, "Serial number").display(), "Not reported");
        assert_eq!(spec(d2, "Type").display(), "DDR4");
        assert_eq!(spec(d2, "Rated speed").display(), "3200 MT/s");
        assert_eq!(spec(d2, "Configured speed").display(), "2933 MT/s");
    }

    #[test]
    fn extended_size_and_truncated_table() {
        let mut d = t17(0x7FFF, 9, 0x22, 0xFFFF, 0, &["A", "B", "C", "D", "E"]);
        // extend with 0x1C extended size = 65536 MB (fits inside the 0x22-byte formatted area)
        d[0x1C..0x20].copy_from_slice(&65536u32.to_le_bytes());
        let dimms = parse_smbios_memory(&d).unwrap();
        assert_eq!(dimms[0].size, Some(64 << 30));
        assert_eq!(dimms[0].speed, None, "extended speed field is beyond this structure's length");
        // truncated mid-structure: nothing complete, so no modules, but no panic
        let full = table();
        assert!(parse_smbios_memory(&full[..40]).is_ok());
    }

    #[test]
    fn garbage_smbios_is_failed_not_panic() {
        let fx = Fx::new(&[("proc/meminfo", MEMINFO)]);
        fx.write_bytes(DMI_REL, &[1, 2, 3]);
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Memory modules").value.availability, Availability::Failed { .. }));
        assert!(parse_smbios_memory(&[0xFF; 64]).is_err());
    }

    #[test]
    fn malformed_meminfo_and_swaps() {
        let fx = Fx::new(&[
            ("proc/meminfo", "MemTotal: lots kB\nMemFree: 100 kB\n"),
            ("proc/swaps", "Filename Type Size Used Priority\n/dev/sda2 partition oops\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "System memory");
        assert!(matches!(spec(c, "Usable memory").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Available memory").display(), "Not reported");
        assert!(matches!(spec(c, "Swap device").value.availability, Availability::Failed { .. }));
    }

    #[test]
    fn no_swap_and_missing_zram_files() {
        let fx = Fx::new(&[("proc/meminfo", MEMINFO), ("proc/swaps", "Filename Type Size Used Priority\n/dev/zram0 partition 1024 0 100\n")]);
        let c = &collect_from(fx.path())[0];
        let z = c.groups.iter().find(|g| g.title == "Swap: /dev/zram0").unwrap();
        assert!(z.specs.iter().any(|s| s.label == "Compression algorithm" && s.display() == "Not reported"));
        let fx2 = Fx::new(&[("proc/meminfo", MEMINFO), ("proc/swaps", "Filename Type Size Used Priority\n")]);
        assert!(collect_from(fx2.path())[0].groups.iter().all(|g| !g.title.starts_with("Swap:")));
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
    fn missing_meminfo_yields_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }
}
