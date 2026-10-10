//! GPUs from PCI sysfs. One component per PCI display-class device (`class` 0x03xxxx).
//!
//! SAFETY: an inspector must not change the power state of what it inspects. Many sysfs attributes of a
//! PCI device (and every DRM connector `status`/`edid`, hwmon file, `mem_info_*`) cause the kernel to
//! runtime-resume the device. A runtime-suspended discrete GPU takes ~18 s to wake and burns power, so the
//! very first thing we do per GPU is read `power/runtime_status`. Unless it is exactly `active`, only
//! the attributes in [`SAFE_WHILE_SUSPENDED`] are read. That list was verified empirically on the
//! reference machine (NVIDIA GB206M, nvidia driver): reading each of them left `runtime_status`
//! at `suspended`, whereas `current_link_speed`, `current_link_width` and `max_link_width` DO wake the
//! device (they read live PCIe config space) and are therefore deliberately NOT on the list.

#[path = "pci_ids.rs"]
pub(crate) mod pci_ids;

use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{at, list_dir, parse_field, read_field, source, text_spec};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

/// Attributes (names relative to the PCI device directory) that are served from the kernel's cached
/// config space and never resume a runtime-suspended device. Everything else is off limits while the
/// device is not `active`. `driver` is a symlink (readlink only) and `drm/` a directory listing.
pub const SAFE_WHILE_SUSPENDED: &[&str] = &[
    "vendor",
    "device",
    "subsystem_vendor",
    "subsystem_device",
    "class",
    "revision",
    "boot_vga",
    "max_link_speed",
    "power_state",
    "power/runtime_status",
];

pub const SUSPENDED_NOTE: &str = "dGPU runtime-suspended; not queried to avoid waking it";

/// Runtime power state of a PCI device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Power {
    /// `runtime_status` is exactly `active`, or the device has no runtime PM file at all (VMs, fixtures
    /// without one): nothing to wake.
    Awake,
    /// Suspended, resuming, unsupported or unreadable: restrict to the safe attribute list.
    Restricted(String),
}

pub(crate) fn power_of(root: &Path, addr: &str) -> Power {
    let rel = format!("sys/bus/pci/devices/{addr}/power/runtime_status");
    match std::fs::read_to_string(at(root, &rel)) {
        Ok(s) if s.trim() == "active" => Power::Awake,
        Ok(s) => Power::Restricted(s.trim().to_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Power::Awake,
        Err(e) => Power::Restricted(format!("unreadable: {e}")),
    }
}

/// Last path component that looks like a PCI address (`0000:01:00.0`).
pub(crate) fn pci_addr_in(path: &str) -> Option<String> {
    path.split('/').rev().find(|c| is_pci_addr(c)).map(str::to_owned)
}

fn is_pci_addr(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12 && b[4] == b':' && b[7] == b':' && b[10] == b'.' && s.chars().enumerate().all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

/// PCI address of the device behind `/sys/class/drm/<card>` (readlink only; touches no hardware).
pub(crate) fn card_pci_addr(root: &Path, card: &str) -> Option<String> {
    let link = std::fs::read_link(at(root, &format!("sys/class/drm/{card}/device"))).ok()?;
    pci_addr_in(&link.to_string_lossy())
}

/// A PCI device plus the gate deciding which attributes may be read.
struct Dev<'a> {
    root: &'a Path,
    addr: &'a str,
    power: Power,
}

impl Dev<'_> {
    fn awake(&self) -> bool {
        self.power == Power::Awake
    }

    fn rel(&self, name: &str) -> String {
        format!("sys/bus/pci/devices/{}/{name}", self.addr)
    }

    fn allowed(&self, name: &str) -> bool {
        self.awake() || SAFE_WHILE_SUSPENDED.contains(&name)
    }

    /// The only way this module reads a device attribute.
    fn attr(&self, name: &str) -> Field<String> {
        let rel = self.rel(name);
        if self.allowed(name) { read_field(self.root, &rel) } else { Field::not_reported(format!("{} ({SUSPENDED_NOTE})", source(&rel))) }
    }

    /// Unavailable-because-suspended placeholder for something outside the PCI attribute set.
    fn withheld(&self, rel: &str) -> Field<Value> {
        Field::not_reported(format!("{} ({SUSPENDED_NOTE})", source(rel)))
    }
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    list_dir(root, "sys/bus/pci/devices")
        .into_iter()
        .filter(|a| is_pci_addr(a))
        .filter(|a| read_field(root, &format!("sys/bus/pci/devices/{a}/class")).value().is_some_and(|c| c.starts_with("0x03")))
        .map(|a| read_gpu(root, &a))
        .collect()
}

fn hex_id(f: &Field<String>) -> Option<String> {
    f.value().map(|s| s.trim_start_matches("0x").to_lowercase())
}

fn short_vendor(v: &str) -> String {
    if let (Some(a), Some(b)) = (v.find('['), v.rfind(']'))
        && a < b
    {
        return v[a + 1..b].to_owned(); // "Advanced Micro Devices, Inc. [AMD/ATI]" -> "AMD/ATI"
    }
    let mut s = v.trim();
    for suf in [" Corporation", " Corp.", ", Inc.", " Inc.", " Limited", " Ltd."] {
        if let Some(t) = s.strip_suffix(suf) {
            s = t;
        }
    }
    s.to_owned()
}

/// Vendors we can name without a pci.ids database (ids are well known).
fn builtin_vendor(id: &str) -> Option<&'static str> {
    match id {
        "8086" => Some("Intel"),
        "10de" => Some("NVIDIA"),
        "1002" => Some("AMD/ATI"),
        "1af4" => Some("Red Hat (virtio)"),
        "15ad" => Some("VMware"),
        "1234" => Some("QEMU"),
        "1414" => Some("Microsoft"),
        "80ee" => Some("VirtualBox"),
        _ => None,
    }
}

fn is_virtual(vendor: &str) -> bool {
    ["1af4", "15ad", "1234", "1414", "80ee", "1b36"].contains(&vendor)
}

/// Honest heuristic: bus 0 is where the CPU/PCH puts integrated graphics; a GPU behind a bridge is
/// almost always a discrete card (but an AMD APU may sit on a non-zero bus, hence "likely").
fn kind(vendor: Option<&str>, addr: &str) -> Option<&'static str> {
    let v = vendor?;
    if is_virtual(v) {
        return Some("Virtual GPU");
    }
    let bus0 = addr.split(':').nth(1) == Some("00");
    Some(match (v, bus0) {
        (_, true) => "Integrated (on the CPU/PCH root bus; heuristic)",
        ("8086", false) | ("10de", false) | ("1002", false) => "Discrete (behind a PCIe bridge; heuristic)",
        _ => return None,
    })
}

fn link_spec(label: &str, f: Field<String>, rated: bool) -> Spec {
    let f = match f.value().map(String::as_str) {
        // Integrated GPUs have no PCIe link; the kernel prints "Unknown" and width 0/255.
        Some("Unknown" | "0" | "255") => Field::not_reported(f.source),
        _ => f,
    };
    let s = text_spec(label, parse_field(f, |v| Ok(v.clone())));
    if rated { s.rated() } else { s }
}

fn width_spec(label: &str, f: Field<String>, rated: bool) -> Spec {
    let mut s = link_spec(label, f, rated);
    if let Some(Value::Text(t)) = s.value.value().cloned() {
        s.value = Field::available(Value::Text(format!("x{t}")), s.value.source.clone());
    }
    s
}

fn read_gpu(root: &Path, addr: &str) -> Component {
    let dev = Dev { root, addr, power: power_of(root, addr) };
    let vendor_id = hex_id(&dev.attr("vendor"));
    let device_id = hex_id(&dev.attr("device"));
    let sub_v = hex_id(&dev.attr("subsystem_vendor"));
    let sub_d = hex_id(&dev.attr("subsystem_device"));
    let sub = sub_v.as_deref().zip(sub_d.as_deref());
    let names = match (&vendor_id, &device_id) {
        (Some(v), Some(d)) => pci_ids::lookup(root, v, d, sub),
        _ => pci_ids::Names::default(),
    };
    let db_note = match names.db {
        Some(p) => format!("file:/{p}"),
        None => "pci.ids database not found (looked in /usr/share/hwdata and /usr/share/misc)".to_owned(),
    };
    let vendor_name = names.vendor.clone().or_else(|| vendor_id.as_deref().and_then(builtin_vendor).map(str::to_owned));
    let vendor_short = vendor_name.as_deref().map(short_vendor);
    let pci_id = vendor_id.as_deref().zip(device_id.as_deref()).map(|(v, d)| format!("{v}:{d}"));

    let device_label = names.device.as_deref().map(|d| match (d.find('['), d.rfind(']')) {
        (Some(a), Some(b)) if a < b => d[a + 1..b].to_owned(),
        _ => d.to_owned(),
    });
    let title = match (&vendor_short, &device_label, &pci_id) {
        (Some(v), Some(d), _) if d.starts_with(v.as_str()) => d.clone(),
        (Some(v), Some(d), _) => format!("{v} {d}"),
        (Some(v), None, Some(id)) => format!("{v} graphics device {id}"),
        (None, None, Some(id)) => format!("Graphics device {id}"),
        _ => format!("Graphics device at {addr}"),
    };

    let driver = std::fs::read_link(at(root, &dev.rel("driver"))).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let driver_spec = match &driver {
        Some(d) => Spec::text("Driver in use", d, &source(&dev.rel("driver"))),
        None => Spec::missing("Driver in use", &source(&dev.rel("driver"))),
    };
    let class_kind = kind(vendor_id.as_deref(), addr);
    let kind_spec = match class_kind {
        Some(k) => Spec::text("Type", k, &source(&dev.rel("vendor"))),
        None => Spec::missing("Type", &source(&dev.rel("vendor"))),
    };
    let primary = parse_field(dev.attr("boot_vga"), |s| match s.as_str() {
        "1" => Ok(Value::Bool(true)),
        "0" => Ok(Value::Bool(false)),
        o => Err(format!("unexpected boot_vga value {o:?}")),
    });
    let runtime = read_field(root, &dev.rel("power/runtime_status"));
    let power_state = dev.attr("power_state");

    // DRM nodes and connected outputs. Directory listings only; connector `status` is read only when awake.
    let nodes: Vec<String> = list_dir(root, &dev.rel("drm")).into_iter().filter(|n| is_node(n)).collect();
    let nodes_spec = if nodes.is_empty() {
        Spec::missing("DRM nodes", &source(&dev.rel("drm")))
    } else {
        Spec::text("DRM nodes", nodes.join(", "), &source(&dev.rel("drm")))
    };
    let outputs = outputs_spec(&dev, &nodes);

    let mut telemetry_title = "Live telemetry".to_owned();
    if !dev.awake() {
        telemetry_title.push_str(" (not queried: GPU suspended)");
    }

    let mut c = Component::new(&format!("graphics:{addr}"), Category::Graphics, &title);
    let sub_parts: Vec<String> = [class_kind.map(|k| k.split(" (").next().unwrap_or(k).to_owned()), driver.clone()].into_iter().flatten().collect();
    if !sub_parts.is_empty() {
        c = c.subtitle(&sub_parts.join(" · "));
    }
    c = c.summary(vec![
        kind_spec,
        driver_spec.clone(),
        Spec::new("Primary display", primary),
        text_spec("Runtime power state", runtime),
    ]);
    c = c.group(SpecGroup::new("Identity", vec![
        match &vendor_name {
            Some(v) => Spec::text("Vendor", v, &db_note),
            None => Spec::missing("Vendor", &db_note),
        },
        match &names.device {
            Some(d) => Spec::text("Device", d, &db_note),
            None => Spec::missing("Device", &db_note),
        },
        match &names.subsystem {
            Some(s) => Spec::text("Subsystem", s, &db_note),
            None => Spec::missing("Subsystem", &db_note),
        },
        match &pci_id {
            Some(id) => Spec::text("PCI ID", id, &source(&dev.rel("device"))),
            None => Spec::missing("PCI ID", &source(&dev.rel("device"))),
        },
        text_spec("Revision", dev.attr("revision")),
    ]));
    c = c.group(SpecGroup::new("Connection", vec![
        Spec::text("Bus address", addr, &source(&format!("sys/bus/pci/devices/{addr}"))),
        link_spec("Current link speed", dev.attr("current_link_speed"), false),
        width_spec("Current link width", dev.attr("current_link_width"), false),
        link_spec("Maximum link speed", dev.attr("max_link_speed"), true),
        width_spec("Maximum link width", dev.attr("max_link_width"), true),
        text_spec("Device power state", power_state),
    ]));
    c = c.group(SpecGroup::new("Display outputs", vec![nodes_spec, outputs]));
    c = c.group(SpecGroup::new(&telemetry_title, telemetry(&dev, driver.as_deref(), nodes.iter().find(|n| n.starts_with("card")).map(String::as_str))));
    c.group(SpecGroup::new("Device", vec![driver_spec, text_spec("PCI class", dev.attr("class"))]).technical())
}

fn is_node(n: &str) -> bool {
    let digits = |p: &str| n.strip_prefix(p).is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    digits("card") || digits("renderD")
}

fn outputs_spec(dev: &Dev, nodes: &[String]) -> Spec {
    let label = "Connected outputs";
    let drm_dir = "sys/class/drm";
    if !dev.awake() {
        return Spec::new(label, dev.withheld(drm_dir));
    }
    let Some(card) = nodes.iter().find(|n| n.starts_with("card")) else {
        return Spec::missing(label, &source(drm_dir));
    };
    let prefix = format!("{card}-");
    let connected: Vec<String> = list_dir(dev.root, drm_dir)
        .into_iter()
        .filter(|n| n.starts_with(&prefix))
        .filter(|n| read_field(dev.root, &format!("{drm_dir}/{n}/status")).value().is_some_and(|s| s == "connected"))
        .map(|n| n[prefix.len()..].to_owned())
        .collect();
    if connected.is_empty() {
        Spec::text(label, "None", &source(&format!("{drm_dir}/{card}-*/status")))
    } else {
        Spec::text(label, connected.join(", "), &source(&format!("{drm_dir}/{card}-*/status")))
    }
}

fn num(f: Field<String>, scale: f64, make: impl Fn(f64) -> Value) -> Field<Value> {
    parse_field(f, |s| s.trim().parse::<f64>().map(|n| make(n * scale)).map_err(|e| e.to_string()))
}

/// Live measurements. Suspended device: every row is a withheld placeholder; no hardware-backed file is opened.
fn telemetry(dev: &Dev, driver: Option<&str>, card: Option<&str>) -> Vec<Spec> {
    let dpath = format!("sys/bus/pci/devices/{}", dev.addr);
    if !dev.awake() {
        return vec![
            Spec::new("Temperature", dev.withheld(&format!("{dpath}/hwmon"))).live(),
            Spec::new("Power draw", dev.withheld(&format!("{dpath}/hwmon"))).live(),
            Spec::new("Video memory used", dev.withheld(&dpath)).live(),
            Spec::new("GPU clock", dev.withheld(&dpath)).live(),
        ];
    }
    let hw = list_dir(dev.root, &format!("{dpath}/hwmon")).into_iter().find(|n| n.starts_with("hwmon"));
    let hwf = |file: &str| match &hw {
        Some(h) => read_field(dev.root, &format!("{dpath}/hwmon/{h}/{file}")),
        None => Field::not_reported(source(&format!("{dpath}/hwmon/hwmon*/{file}"))),
    };
    let mut v = vec![
        Spec::new("Temperature", num(hwf("temp1_input"), 0.001, Value::Celsius)).live(),
        Spec::new("Power draw", {
            let avg = hwf("power1_average");
            num(if avg.value().is_some() { avg } else { hwf("power1_input") }, 1e-6, Value::Watts)
        })
        .live(),
    ];
    if hw.as_ref().is_some_and(|h| at(dev.root, &format!("{dpath}/hwmon/{h}/fan1_input")).exists()) {
        v.push(Spec::new("Fan speed", num(hwf("fan1_input"), 1.0, |n| Value::Rpm(n as u64))).live());
    }
    match driver {
        Some("amdgpu") => {
            let rd = |f: &str| read_field(dev.root, &format!("{dpath}/{f}"));
            v.push(Spec::new("Video memory used", num(rd("mem_info_vram_used"), 1.0, |n| Value::Bytes(n as u64))).live());
            v.push(Spec::new("Video memory total", num(rd("mem_info_vram_total"), 1.0, |n| Value::Bytes(n as u64))));
            v.push(Spec::new("GPU load", num(rd("gpu_busy_percent"), 1.0, Value::Percent)).live());
            let sclk = parse_field(rd("pp_dpm_sclk"), |t| {
                let line = t.lines().find(|l| l.trim_end().ends_with('*')).ok_or("no active clock level marked")?;
                let mhz = line.split_whitespace().nth(1).and_then(|w| w.to_lowercase().strip_suffix("mhz").and_then(|n| n.parse::<f64>().ok())).ok_or("unparseable clock level")?;
                Ok(Value::Hertz((mhz * 1e6) as u64))
            });
            v.push(Spec::new("GPU clock", sclk).live());
        }
        Some("i915") => {
            if let Some(card) = card {
                let rd = |f: &str| read_field(dev.root, &format!("sys/class/drm/{card}/{f}"));
                v.push(Spec::new("GPU clock", num(rd("gt_cur_freq_mhz"), 1e6, |n| Value::Hertz(n as u64))).live());
                v.push(Spec::new("Maximum GPU clock", num(rd("gt_RP0_freq_mhz"), 1e6, |n| Value::Hertz(n as u64))).rated());
            }
        }
        Some("nvidia") => v.push(Spec::missing("Video memory and clocks", "NVIDIA proprietary driver: no sysfs telemetry; NVML is not used by design")),
        _ => {}
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    const IDS: &str = "10de  NVIDIA Corporation\n\t2d18  GB206M [GeForce RTX 5070 Max-Q / Mobile]\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t744c  Navi 31 [Radeon RX 7900 XT/7900 XTX]\n17aa  Lenovo\n8086  Intel Corporation\n\ta788  Raptor Lake-S UHD Graphics\n";
    const UNSAFE: &str = "SENTINEL-UNSAFE";

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    fn igpu(fx: &Fx) {
        let d = "sys/bus/pci/devices/0000:00:02.0";
        for (f, v) in [
            ("class", "0x030000"), ("vendor", "0x8086"), ("device", "0xa788"), ("subsystem_vendor", "0x17aa"), ("subsystem_device", "0x3e31"),
            ("revision", "0x04"), ("boot_vga", "1"), ("current_link_speed", "Unknown"), ("current_link_width", "0"), ("max_link_speed", "Unknown"),
            ("max_link_width", "255"), ("power/runtime_status", "active"), ("power_state", "D0"),
        ] {
            fx.write(&format!("{d}/{f}"), &format!("{v}\n"));
        }
        fx.symlink(&format!("{d}/driver"), "../../../bus/pci/drivers/i915");
        fx.write(&format!("{d}/drm/card1/uevent"), "");
        fx.write(&format!("{d}/drm/renderD128/uevent"), "");
        fx.write(&format!("{d}/drm/controlD65/uevent"), "");
        fx.symlink("sys/class/drm/card1/device", "../../../0000:00:02.0");
        fx.write("sys/class/drm/card1/gt_cur_freq_mhz", "700\n");
        fx.write("sys/class/drm/card1/gt_RP0_freq_mhz", "1650\n");
        fx.write("sys/class/drm/card1-eDP-1/status", "connected\n");
        fx.write("sys/class/drm/card1-DP-1/status", "disconnected\n");
        fx.write("usr/share/hwdata/pci.ids", IDS);
    }

    fn dgpu(fx: &Fx, status: &str, value: &str) {
        let d = "sys/bus/pci/devices/0000:01:00.0";
        for (f, v) in [
            ("class", "0x030000"), ("vendor", "0x10de"), ("device", "0x2d18"), ("subsystem_vendor", "0x17aa"), ("subsystem_device", "0x3e31"),
            ("revision", "0xa1"), ("boot_vga", "0"), ("max_link_speed", "32.0 GT/s PCIe"), ("power/runtime_status", status), ("power_state", "D3cold"),
        ] {
            fx.write(&format!("{d}/{f}"), &format!("{v}\n"));
        }
        // Everything below wakes a suspended GPU in real life; the fixture value is a tripwire.
        for f in ["current_link_speed", "current_link_width", "max_link_width", "hwmon/hwmon3/temp1_input", "hwmon/hwmon3/power1_average", "mem_info_vram_used", "pp_dpm_sclk"] {
            fx.write(&format!("{d}/{f}"), value);
        }
        fx.symlink(&format!("{d}/driver"), "../../../bus/pci/drivers/nvidia");
        fx.write(&format!("{d}/drm/card0/uevent"), UNSAFE);
        fx.symlink("sys/class/drm/card0/device", "../../../0000:01:00.0");
        fx.write("sys/class/drm/card0-HDMI-A-1/status", UNSAFE);
    }

    fn dump(c: &Component) -> String {
        format!("{c:?}")
    }

    #[test]
    fn intel_igpu_integrated_primary_with_outputs_and_clock() {
        let fx = Fx::new(&[]);
        igpu(&fx);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "graphics:0000:00:02.0");
        assert_eq!(c.category, Category::Graphics);
        assert_eq!(c.title, "Intel Raptor Lake-S UHD Graphics");
        assert_eq!(spec(c, "Driver in use").display(), "i915");
        assert_eq!(spec(c, "Primary display").display(), "Yes");
        assert!(spec(c, "Type").display().starts_with("Integrated"));
        assert_eq!(spec(c, "DRM nodes").display(), "card1, renderD128");
        assert_eq!(spec(c, "Connected outputs").display(), "eDP-1");
        assert_eq!(spec(c, "GPU clock").display(), "700 MHz");
        assert!(spec(c, "GPU clock").live);
        assert!(spec(c, "Maximum GPU clock").rated);
        assert_eq!(spec(c, "Subsystem").display(), "Lenovo device [17aa:3e31]");
        // The iGPU has no PCIe link: honest "Not reported", not "Unknown"/0/255.
        assert_eq!(spec(c, "Current link speed").display(), "Not reported");
        assert_eq!(spec(c, "Maximum link width").display(), "Not reported");
    }

    #[test]
    fn suspended_dgpu_reads_only_the_allowed_attributes() {
        let fx = Fx::new(&[]);
        igpu(&fx);
        dgpu(&fx, "suspended", UNSAFE);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 2);
        let c = v.iter().find(|c| c.id == "graphics:0000:01:00.0").unwrap();
        assert!(!dump(c).contains(UNSAFE), "tripwire value leaked: a wake-capable attribute was read");
        assert_eq!(c.title, "NVIDIA GeForce RTX 5070 Max-Q / Mobile");
        assert_eq!(spec(c, "Runtime power state").display(), "suspended");
        assert_eq!(spec(c, "Driver in use").display(), "nvidia");
        assert_eq!(spec(c, "Primary display").display(), "No");
        assert!(spec(c, "Type").display().starts_with("Discrete"));
        // The allowed ones are shown...
        assert_eq!(spec(c, "Maximum link speed").display(), "32.0 GT/s PCIe");
        assert!(spec(c, "Maximum link speed").rated);
        assert_eq!(spec(c, "Device power state").display(), "D3cold");
        // ...the rest is explicitly withheld with the reason in the source.
        for l in ["Current link speed", "Current link width", "Maximum link width", "Temperature", "Power draw", "Video memory used", "GPU clock", "Connected outputs"] {
            let s = spec(c, l);
            assert!(matches!(s.value.availability, Availability::NotReported), "{l}");
            assert!(s.value.source.contains(SUSPENDED_NOTE), "{l}: {}", s.value.source);
        }
        assert!(c.groups.iter().any(|g| g.title.contains("not queried: GPU suspended")));
    }

    #[test]
    fn allow_list_is_explicit_and_excludes_wake_attributes() {
        for wake in ["current_link_speed", "current_link_width", "max_link_width", "resource", "config", "enable"] {
            assert!(!SAFE_WHILE_SUSPENDED.contains(&wake), "{wake} must stay off the list");
        }
        let fx = Fx::new(&[]);
        dgpu(&fx, "suspended", UNSAFE);
        let d = Dev { root: fx.path(), addr: "0000:01:00.0", power: power_of(fx.path(), "0000:01:00.0") };
        assert!(!d.awake());
        for a in SAFE_WHILE_SUSPENDED {
            assert!(d.allowed(a));
        }
        assert!(!d.allowed("current_link_speed"));
        assert_eq!(d.attr("current_link_speed").value(), None);
    }

    #[test]
    fn any_non_active_state_is_restricted() {
        for st in ["suspended", "suspending", "resuming", "error", "unsupported"] {
            let fx = Fx::new(&[]);
            dgpu(&fx, st, UNSAFE);
            let c = &collect_from(fx.path())[0];
            assert!(!dump(c).contains(UNSAFE), "{st}");
        }
    }

    #[test]
    fn active_dgpu_reports_link_and_hwmon_telemetry() {
        let fx = Fx::new(&[]);
        dgpu(&fx, "active", "32.0 GT/s PCIe");
        fx.write("sys/bus/pci/devices/0000:01:00.0/current_link_width", "8\n");
        fx.write("sys/bus/pci/devices/0000:01:00.0/max_link_width", "16\n");
        fx.write("sys/bus/pci/devices/0000:01:00.0/hwmon/hwmon3/temp1_input", "43000\n");
        fx.write("sys/bus/pci/devices/0000:01:00.0/hwmon/hwmon3/power1_average", "12500000\n");
        fx.write("usr/share/hwdata/pci.ids", IDS);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Current link width").display(), "x8");
        assert_eq!(spec(c, "Maximum link width").display(), "x16");
        assert!(spec(c, "Maximum link width").rated && !spec(c, "Current link width").rated);
        assert_eq!(spec(c, "Temperature").display(), "43 °C");
        assert_eq!(spec(c, "Power draw").display(), "12.5 W");
        assert!(spec(c, "Temperature").live);
        // NVIDIA has no sysfs VRAM/clock: reported honestly.
        assert_eq!(spec(c, "Video memory and clocks").display(), "Not reported");
    }

    #[test]
    fn amdgpu_telemetry_when_active() {
        let fx = Fx::new(&[
            ("sys/bus/pci/devices/0000:03:00.0/class", "0x030000\n"),
            ("sys/bus/pci/devices/0000:03:00.0/vendor", "0x1002\n"),
            ("sys/bus/pci/devices/0000:03:00.0/device", "0x744c\n"),
            ("sys/bus/pci/devices/0000:03:00.0/power/runtime_status", "active\n"),
            ("sys/bus/pci/devices/0000:03:00.0/mem_info_vram_total", "21458059264\n"),
            ("sys/bus/pci/devices/0000:03:00.0/mem_info_vram_used", "1073741824\n"),
            ("sys/bus/pci/devices/0000:03:00.0/gpu_busy_percent", "37\n"),
            ("sys/bus/pci/devices/0000:03:00.0/pp_dpm_sclk", "0: 500Mhz\n1: 1500Mhz *\n2: 2500Mhz\n"),
            ("usr/share/hwdata/pci.ids", IDS),
        ]);
        fx.symlink("sys/bus/pci/devices/0000:03:00.0/driver", "../../../bus/pci/drivers/amdgpu");
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "AMD/ATI Radeon RX 7900 XT/7900 XTX");
        assert_eq!(spec(c, "Video memory used").display(), "1.1 GB");
        assert_eq!(spec(c, "GPU load").display(), "37%");
        assert_eq!(spec(c, "GPU clock").display(), "1.50 GHz");
    }

    #[test]
    fn no_pci_ids_falls_back_to_hex_ids_honestly() {
        let fx = Fx::new(&[]);
        dgpu(&fx, "suspended", UNSAFE);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "NVIDIA graphics device 10de:2d18");
        assert_eq!(spec(c, "Device").display(), "Not reported");
        assert!(spec(c, "Device").value.source.contains("pci.ids database not found"));
        assert_eq!(spec(c, "PCI ID").display(), "10de:2d18");
    }

    #[test]
    fn vm_virtio_gpu_without_runtime_pm_or_link() {
        let fx = Fx::new(&[
            ("sys/bus/pci/devices/0000:00:01.0/class", "0x030000\n"),
            ("sys/bus/pci/devices/0000:00:01.0/vendor", "0x1af4\n"),
            ("sys/bus/pci/devices/0000:00:01.0/device", "0x1050\n"),
            ("sys/bus/pci/devices/0000:00:01.0/drm/card0/uevent", ""),
            ("sys/class/drm/card0-Virtual-1/status", "connected\n"),
        ]);
        fx.symlink("sys/bus/pci/devices/0000:00:01.0/driver", "../../../bus/pci/drivers/virtio-pci");
        let v = collect_from(fx.path());
        let c = &v[0];
        assert_eq!(c.title, "Red Hat (virtio) graphics device 1af4:1050");
        assert_eq!(spec(c, "Type").display(), "Virtual GPU");
        assert_eq!(spec(c, "Primary display").display(), "Not reported");
        assert_eq!(spec(c, "Runtime power state").display(), "Not reported");
        assert_eq!(spec(c, "Current link speed").display(), "Not reported");
        assert_eq!(spec(c, "Connected outputs").display(), "Virtual-1");
    }

    #[test]
    fn non_display_devices_and_empty_roots_are_ignored() {
        let fx = Fx::new(&[("sys/bus/pci/devices/0000:00:14.0/class", "0x0c0330\n")]);
        assert!(collect_from(fx.path()).is_empty());
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    #[test]
    fn pci_address_extraction() {
        assert_eq!(pci_addr_in("../../../0000:00:02.0").as_deref(), Some("0000:00:02.0"));
        assert_eq!(pci_addr_in("/sys/devices/pci0000:00/0000:00:01.0/virtio0").as_deref(), Some("0000:00:01.0"));
        assert_eq!(pci_addr_in("/sys/devices/platform/simple-framebuffer.0"), None);
    }

    #[test]
    #[ignore = "reads the live machine; run with --ignored --nocapture"]
    fn print_live_gpus() {
        for c in collect() {
            println!("== {} [{}] {:?}", c.title, c.id, c.subtitle);
            for g in &c.groups {
                println!("  -- {}", g.title);
                for s in &g.specs {
                    println!("     {}: {}   <{}>", s.label, s.display(), s.value.source);
                }
            }
        }
    }
}
