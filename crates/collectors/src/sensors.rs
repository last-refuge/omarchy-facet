//! hwmon chips from `/sys/class/hwmon`, one component per chip. Read-only; values are converted
//! from the kernel's milli-units exactly (millidegrees C, mV, µW, mA). Implausible or failed reads are
//! reported as such, never as zero. Chips behind a PCI display-class device are skipped (the graphics
//! collector owns GPU telemetry) and chips behind any runtime-suspended PCI device are not read,
//! because reading them can wake the device.

use std::fs;
use std::path::{Path, PathBuf};

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{at, list_dir, parse_field, read_field, source};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let mut nodes: Vec<(u32, String)> = list_dir(root, "sys/class/hwmon")
        .into_iter()
        .filter_map(|n| n.strip_prefix("hwmon").and_then(|d| d.parse::<u32>().ok()).map(|i| (i, n.clone())))
        .collect();
    nodes.sort();
    nodes.into_iter().filter_map(|(_, n)| read_chip(root, &n)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Guard {
    Clear,
    /// Behind a PCI device that is runtime-suspended: do not touch.
    Suspended,
    /// Behind a PCI display-class device: owned by the graphics collector.
    Display,
}

/// Inspect the PCI ancestors of a resolved sysfs device path without reading any hardware
/// (only the `class` and `power/runtime_status` attributes, which never wake a device).
pub(crate) fn guard(root: &Path, real: &Path) -> Guard {
    let stop = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut result = Guard::Clear;
    for a in real.ancestors() {
        if a == stop || !a.starts_with(&stop) {
            break;
        }
        let Ok(class) = fs::read_to_string(a.join("class")) else { continue };
        if class.trim().starts_with("0x03") {
            return Guard::Display;
        }
        if result == Guard::Clear {
            let status = fs::read_to_string(a.join("power/runtime_status")).unwrap_or_default();
            if status.trim() == "suspended" {
                result = Guard::Suspended;
            }
        }
    }
    result
}

/// Millidegrees C -> Celsius. Rejects the values drivers use for "no sensor".
pub(crate) fn milli_temp(f: Field<String>) -> Field<Value> {
    let src = f.source.clone();
    let parsed = parse_field(f, |s| s.parse::<i64>().map_err(|e| format!("not a number: {e}")));
    match parsed.availability {
        facet_core::Availability::Available { value: m } => {
            if m == 0 {
                Field::not_reported(format!("{src} (reads 0: treated as an unconnected sensor)"))
            } else if m <= -273_000 || m > 200_000 {
                Field::failed(format!("implausible reading {:.1} °C ignored", m as f64 / 1000.0), src)
            } else {
                Field::available(Value::Celsius(m as f64 / 1000.0), src)
            }
        }
        _ => parse_field(parsed, |_| Err(String::new())),
    }
}

fn num(root: &Path, rel: &str) -> Field<i64> {
    parse_field(read_field(root, rel), |s| s.parse::<i64>().map_err(|e| format!("not a number: {e}")))
}

fn scaled(root: &Path, rel: &str, div: f64, zero_unreported: bool, make: impl FnOnce(f64) -> Value) -> Field<Value> {
    let f = num(root, rel);
    if zero_unreported && f.value() == Some(&0) {
        return Field::not_reported(format!("{} (reads 0)", f.source));
    }
    parse_field(f, |v| Ok(make(*v as f64 / div)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Temp,
    Fan,
    Volt,
    Power,
    Curr,
}

impl Kind {
    const ALL: [Kind; 5] = [Kind::Temp, Kind::Fan, Kind::Volt, Kind::Power, Kind::Curr];

    fn prefix(self) -> &'static str {
        match self {
            Kind::Temp => "temp",
            Kind::Fan => "fan",
            Kind::Volt => "in",
            Kind::Power => "power",
            Kind::Curr => "curr",
        }
    }

    fn group(self) -> &'static str {
        match self {
            Kind::Temp => "Temperatures",
            Kind::Fan => "Fans",
            Kind::Volt => "Voltages",
            Kind::Power => "Power",
            Kind::Curr => "Current",
        }
    }

    fn default_label(self, n: u32) -> String {
        match self {
            Kind::Temp => format!("Temperature {n}"),
            Kind::Fan => format!("Fan {n}"),
            Kind::Volt => format!("Voltage in{n}"),
            Kind::Power => format!("Power {n}"),
            Kind::Curr => format!("Current {n}"),
        }
    }
}

/// Indexes `n` for which `<prefix><n>_input` exists, ascending.
fn inputs(names: &[String], kind: Kind) -> Vec<u32> {
    let mut v: Vec<u32> = names
        .iter()
        .filter_map(|f| {
            let rest = f.strip_prefix(kind.prefix())?.strip_suffix("_input")?;
            if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) { None } else { rest.parse().ok() }
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn amps_text(a: f64) -> Value {
    Value::Text(format!("{a:.3} A"))
}

fn friendly_title(name: &str) -> String {
    match name {
        "coretemp" => "CPU temperature sensors".into(),
        "nvme" => "NVMe drive sensors".into(),
        "spd5118" => "Memory module sensor (SPD5118)".into(),
        "lenovo_wmi_other" => "Lenovo fan and thermal controller".into(),
        "ADP0" => "AC adapter sensors".into(),
        "BAT0" => "Battery sensors".into(),
        n if n.starts_with("mt79") || n.starts_with("iwlwifi") || n.starts_with("mt76") => format!("Wi-Fi adapter sensors ({n})"),
        n if n.starts_with("ucsi_") => format!("USB-C port power sensors ({n})"),
        n => format!("{n} sensors"),
    }
}

/// NVMe namespace component id for a controller (`sys/class/nvme/<ctrl>/<ctrl>n<k>`), if determinable.
fn nvme_parent(root: &Path, hwmon: &str) -> Option<String> {
    let dev = fs::canonicalize(at(root, &format!("sys/class/hwmon/{hwmon}/device"))).ok()?;
    let ctrl = dev.file_name()?.to_string_lossy().into_owned();
    if !ctrl.starts_with("nvme") {
        return None;
    }
    list_dir(root, &format!("sys/class/nvme/{ctrl}"))
        .into_iter()
        .find(|e| e.strip_prefix(&format!("{ctrl}n")).is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())))
        .map(|ns| format!("storage:{ns}"))
}

fn read_chip(root: &Path, node: &str) -> Option<Component> {
    let base = format!("sys/class/hwmon/{node}");
    let real: Option<PathBuf> = fs::canonicalize(at(root, &base)).ok();
    let g = real.as_deref().map_or(Guard::Clear, |r| guard(root, r));
    if g == Guard::Display {
        return None;
    }
    let name = read_field(root, &format!("{base}/name")).value().cloned().unwrap_or_else(|| node.to_owned());
    let id = format!("sensors:{name}-{node}");
    let mut c = Component::new(&id, Category::Power, &friendly_title(&name));
    if let Some(p) = (name == "nvme").then(|| nvme_parent(root, node)).flatten() {
        c = c.parent(&p);
    }

    let technical = SpecGroup::new(
        "Chip",
        vec![
            Spec::text("Chip name", &name, &source(&format!("{base}/name"))),
            Spec::text("Kernel node", node, &source(&base)),
        ],
    )
    .technical();

    if g == Guard::Suspended {
        return Some(
            c.subtitle("Not queried: device is runtime-suspended")
                .group(SpecGroup::new(
                    "Status",
                    vec![Spec::missing("Readings", &format!("{} (parent PCI device is runtime-suspended; not read to avoid waking it)", source(&base)))],
                ))
                .group(technical),
        );
    }

    let names = list_dir(root, &base);
    let mut groups = Vec::new();
    let mut summary: Vec<Spec> = Vec::new();
    let mut count = 0usize;
    for kind in Kind::ALL {
        let mut specs = Vec::new();
        for n in inputs(&names, kind) {
            let p = kind.prefix();
            let rel = |suffix: &str| format!("{base}/{p}{n}_{suffix}");
            let label = read_field(root, &rel("label")).value().cloned().unwrap_or_else(|| kind.default_label(n));
            let reading = match kind {
                Kind::Temp => milli_temp(read_field(root, &rel("input"))),
                Kind::Fan => parse_field(num(root, &rel("input")), |v| {
                    u64::try_from(*v).map(Value::Rpm).map_err(|_| format!("negative fan speed {v}"))
                }),
                Kind::Volt => scaled(root, &rel("input"), 1000.0, false, Value::Volts),
                Kind::Power => scaled(root, &rel("input"), 1_000_000.0, false, Value::Watts),
                Kind::Curr => scaled(root, &rel("input"), 1000.0, false, amps_text),
            };
            let spec = Spec::new(&label, reading).live();
            if summary.len() < 3 && (spec.is_available()) && !summary.iter().any(|s| s.value.value().map(std::mem::discriminant) == spec.value.value().map(std::mem::discriminant)) {
                summary.push(spec.clone());
            }
            specs.push(spec);
            count += 1;
            match kind {
                Kind::Temp => {
                    for (suffix, what) in [("crit", "critical limit"), ("max", "high limit")] {
                        if names.iter().any(|f| *f == format!("{p}{n}_{suffix}")) {
                            specs.push(Spec::new(format!("{label} ({what})"), milli_temp(read_field(root, &rel(suffix)))).rated());
                        }
                    }
                }
                Kind::Volt => {
                    for (suffix, what) in [("min", "minimum"), ("max", "maximum")] {
                        if names.iter().any(|f| *f == format!("{p}{n}_{suffix}")) {
                            specs.push(Spec::new(format!("{label} ({what})"), scaled(root, &rel(suffix), 1000.0, true, Value::Volts)).rated());
                        }
                    }
                }
                Kind::Curr if names.iter().any(|f| *f == format!("{p}{n}_max")) => {
                    specs.push(Spec::new(format!("{label} (maximum)"), scaled(root, &rel("max"), 1000.0, true, amps_text)).rated());
                }
                _ => {}
            }
        }
        if !specs.is_empty() {
            groups.push(SpecGroup::new(kind.group(), specs));
        }
    }
    if count == 0 {
        return None;
    }
    c = c.subtitle(&format!("{count} reading{}", if count == 1 { "" } else { "s" })).summary(summary);
    for gr in groups {
        c = c.group(gr);
    }
    Some(c.group(technical))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}: {:?}", c.all_specs().map(|s| &s.label).collect::<Vec<_>>()))
    }

    #[test]
    fn converts_milli_units_exactly() {
        let fx = Fx::new(&[
            ("sys/class/hwmon/hwmon3/name", "board\n"),
            ("sys/class/hwmon/hwmon3/temp1_input", "41500\n"),
            ("sys/class/hwmon/hwmon3/temp1_label", "Package id 0\n"),
            ("sys/class/hwmon/hwmon3/temp1_crit", "100000\n"),
            ("sys/class/hwmon/hwmon3/temp1_max", "90000\n"),
            ("sys/class/hwmon/hwmon3/fan1_input", "2310\n"),
            ("sys/class/hwmon/hwmon3/in0_input", "15745\n"),
            ("sys/class/hwmon/hwmon3/power1_input", "56619000\n"),
            ("sys/class/hwmon/hwmon3/curr1_input", "5000\n"),
            ("sys/class/hwmon/hwmon3/in0_max", "0\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "sensors:board-hwmon3");
        assert_eq!(c.category, Category::Power);
        assert_eq!(spec(c, "Package id 0").value.value(), Some(&Value::Celsius(41.5)));
        assert!(spec(c, "Package id 0").live);
        assert_eq!(spec(c, "Package id 0 (critical limit)").value.value(), Some(&Value::Celsius(100.0)));
        assert!(spec(c, "Package id 0 (critical limit)").rated);
        assert_eq!(spec(c, "Fan 1").value.value(), Some(&Value::Rpm(2310)));
        assert_eq!(spec(c, "Voltage in0").value.value(), Some(&Value::Volts(15.745)));
        assert_eq!(spec(c, "Power 1").value.value(), Some(&Value::Watts(56.619)));
        assert_eq!(spec(c, "Current 1").display(), "5.000 A");
        assert_eq!(spec(c, "Voltage in0 (maximum)").display(), "Not reported");
        assert!(!c.summary.is_empty());
    }

    #[test]
    fn bogus_and_failed_readings_are_not_zero() {
        let fx = Fx::new(&[
            ("sys/class/hwmon/hwmon0/name", "nvme\n"),
            ("sys/class/hwmon/hwmon0/temp1_input", "-273000\n"),
            ("sys/class/hwmon/hwmon0/temp2_input", "0\n"),
            ("sys/class/hwmon/hwmon0/temp3_input", "garbage\n"),
            ("sys/class/hwmon/hwmon0/temp3_max", "65261800\n"),
            ("sys/class/hwmon/hwmon0/temp4_input", "-5000\n"),
            ("sys/class/hwmon/hwmon0/fan1_input", "0\n"),
            ("sys/class/hwmon/hwmon0/fan2_input", "-1\n"),
        ]);
        // temp5 is a directory: reading it fails with a non-NotFound I/O error (like EIO).
        std::fs::create_dir_all(fx.path().join("sys/class/hwmon/hwmon0/temp5_input")).unwrap();
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Temperature 1").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Temperature 2").value.availability, Availability::NotReported));
        assert!(matches!(spec(c, "Temperature 3").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Temperature 3 (high limit)").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Temperature 4").value.value(), Some(&Value::Celsius(-5.0)));
        assert!(matches!(spec(c, "Temperature 5").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Fan 1").value.value(), Some(&Value::Rpm(0)));
        assert!(matches!(spec(c, "Fan 2").value.availability, Availability::Failed { .. }));
    }

    #[test]
    fn chip_without_name_or_labels_and_empty_chips() {
        let fx = Fx::new(&[
            ("sys/class/hwmon/hwmon1/temp1_input", "30000\n"),
            ("sys/class/hwmon/hwmon2/name", "hidpp_battery_0\n"),
            ("sys/class/hwmon/hwmon10/name", "ADP0\n"),
            ("sys/class/hwmon/hwmon10/in0_input", "1000\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 2, "chip with no readings is omitted");
        assert_eq!(v[0].id, "sensors:hwmon1-hwmon1");
        assert_eq!(spec(&v[0], "Temperature 1").display(), "30 °C");
        assert_eq!(v[1].id, "sensors:ADP0-hwmon10");
    }

    #[test]
    fn display_class_gpu_is_skipped_and_suspended_pci_is_not_read() {
        let fx = Fx::new(&[
            ("sys/devices/pci0000:00/0000:01:00.0/class", "0x030200\n"),
            ("sys/devices/pci0000:00/0000:01:00.0/power/runtime_status", "suspended\n"),
            ("sys/devices/pci0000:00/0000:01:00.0/hwmon/hwmon0/name", "nvidia\n"),
            ("sys/devices/pci0000:00/0000:01:00.0/hwmon/hwmon0/temp1_input", "50000\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/class", "0x010802\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/power/runtime_status", "suspended\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2/name", "nvme\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2/temp1_input", "40000\n"),
            ("sys/devices/pci0000:00/0000:07:00.0/class", "0x028000\n"),
            ("sys/devices/pci0000:00/0000:07:00.0/power/runtime_status", "active\n"),
            ("sys/devices/pci0000:00/0000:07:00.0/hwmon/hwmon3/name", "mt7925_phy0\n"),
            ("sys/devices/pci0000:00/0000:07:00.0/hwmon/hwmon3/temp1_input", "52000\n"),
        ]);
        fx.symlink("sys/class/hwmon/hwmon0", "../../devices/pci0000:00/0000:01:00.0/hwmon/hwmon0");
        fx.symlink("sys/class/hwmon/hwmon2", "../../devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2");
        fx.symlink("sys/class/hwmon/hwmon3", "../../devices/pci0000:00/0000:07:00.0/hwmon/hwmon3");
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "sensors:nvme-hwmon2");
        assert!(v[0].all_specs().all(|s| !s.live), "suspended device is not read");
        assert!(matches!(spec(&v[0], "Readings").value.availability, Availability::NotReported));
        assert_eq!(v[1].id, "sensors:mt7925_phy0-hwmon3");
        assert_eq!(spec(&v[1], "Temperature 1").display(), "52 °C");
    }

    #[test]
    fn nvme_parent_resolved_through_device_link() {
        let fx = Fx::new(&[
            ("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2/name", "nvme\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2/temp1_input", "40000\n"),
            ("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/nvme0n1/size", "1\n"),
        ]);
        fx.symlink("sys/class/hwmon/hwmon2", "../../devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2");
        fx.symlink("sys/devices/pci0000:00/0000:06:00.0/nvme/nvme0/hwmon2/device", "..");
        fx.symlink("sys/class/nvme/nvme0", "../../devices/pci0000:00/0000:06:00.0/nvme/nvme0");
        let v = collect_from(fx.path());
        assert_eq!(v[0].parent.as_deref(), Some("storage:nvme0n1"));
    }

    #[test]
    fn no_hwmon_is_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }
}
