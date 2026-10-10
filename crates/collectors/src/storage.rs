//! Physical block devices from sysfs. Partitions and logical devices (dm, zram, loop, md) are excluded.

use std::fs;
use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{list_dir, parse_field, read_field, source, text_spec};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    list_dir(root, "sys/block").into_iter().filter(|n| is_physical(n)).map(|n| read_device(root, &n)).collect()
}

fn is_physical(name: &str) -> bool {
    !["dm-", "zram", "loop", "ram", "md", "sr", "fd", "nbd"].iter().any(|p| name.starts_with(p))
}

fn parse_flag(s: &String) -> Result<bool, String> {
    match s.as_str() {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(format!("unexpected flag value {other:?}")),
    }
}

/// Interface name from the device name and the (symlink) path of its sysfs entry.
pub fn transport(root: &Path, name: &str) -> Option<&'static str> {
    if name.starts_with("nvme") {
        return Some("NVMe");
    }
    if name.starts_with("mmcblk") {
        return Some("SD/eMMC");
    }
    if name.starts_with("vd") {
        return Some("VirtIO");
    }
    let link = fs::read_link(crate::util::at(root, &format!("sys/block/{name}"))).ok()?;
    let p = link.to_string_lossy();
    if p.contains("/usb") {
        Some("USB")
    } else if p.contains("/ata") {
        Some("SATA")
    } else if p.contains("/virtio") {
        Some("VirtIO")
    } else {
        None
    }
}

fn media_type(rotational: Option<bool>, transport: Option<&str>) -> Option<&'static str> {
    match (rotational, transport) {
        (_, Some("NVMe")) => Some("NVMe solid-state drive"),
        (Some(true), _) => Some("Hard disk drive"),
        (Some(false), Some("SD/eMMC")) => Some("Flash storage"),
        (Some(false), _) => Some("Solid-state drive"),
        _ => None,
    }
}

fn read_device(root: &Path, name: &str) -> Component {
    let dev = format!("sys/block/{name}/device");
    let model = read_field(root, &format!("{dev}/model"));
    let vendor = read_field(root, &format!("{dev}/vendor"));
    let serial = read_field(root, &format!("{dev}/serial"));
    let firmware = {
        let f = read_field(root, &format!("{dev}/firmware_rev"));
        if f.value().is_some() { f } else { read_field(root, &format!("{dev}/rev")) }
    };
    // sysfs `size` is always in 512-byte sectors regardless of the device's sector size.
    let capacity = parse_field(read_field(root, &format!("sys/block/{name}/size")), |s| {
        s.parse::<u64>().map(|n| Value::Bytes(n.saturating_mul(512))).map_err(|e| e.to_string())
    });
    let rotational = parse_field(read_field(root, &format!("sys/block/{name}/queue/rotational")), parse_flag);
    let removable = parse_field(read_field(root, &format!("sys/block/{name}/removable")), parse_flag);
    let iface = transport(root, name);
    let media = media_type(rotational.value().copied(), iface);

    let title = model.value().cloned().unwrap_or_else(|| format!("Storage device {name}"));
    let subtitle = [capacity.value().map(Value::display), media.map(str::to_owned)].into_iter().flatten().collect::<Vec<_>>().join(" · ");

    let bool_spec = |label: &str, f: Field<bool>| Spec::new(label, parse_field(f, |b| Ok(Value::Bool(*b))));
    let mut c = Component::new(&format!("storage:{name}"), Category::Storage, &title);
    if !subtitle.is_empty() {
        c = c.subtitle(&subtitle);
    }
    c.summary(vec![
        Spec::new("Capacity", capacity),
        match media {
            Some(m) => Spec::text("Type", m, &source(&format!("sys/block/{name}/queue/rotational"))),
            None => Spec::missing("Type", &source(&format!("sys/block/{name}/queue/rotational"))),
        },
        match iface {
            Some(i) => Spec::text("Interface", i, &source(&format!("sys/block/{name}"))),
            None => Spec::missing("Interface", &source(&format!("sys/block/{name}"))),
        },
    ])
    .group(SpecGroup::new("Identity", vec![
        text_spec("Model", model),
        text_spec("Vendor", vendor),
        text_spec("Firmware", firmware),
        text_spec("Serial number", serial).identifier(),
    ]))
    .group(SpecGroup::new("Device", vec![
        Spec::text("Kernel name", name, &source(&format!("sys/block/{name}"))),
        bool_spec("Removable", removable),
        bool_spec("Rotational", rotational),
    ]).technical())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    #[test]
    fn nvme_drive_with_logical_devices_skipped() {
        let fx = Fx::new(&[
            ("sys/block/nvme0n1/device/model", "SAMSUNG TEST   \n"),
            ("sys/block/nvme0n1/device/firmware_rev", "4L1QKXF7\n"),
            ("sys/block/nvme0n1/device/serial", "S0000EXAMPLE0001\n"),
            ("sys/block/nvme0n1/size", "2000409264\n"),
            ("sys/block/nvme0n1/queue/rotational", "0\n"),
            ("sys/block/nvme0n1/removable", "0\n"),
            ("sys/block/dm-0/size", "100\n"),
            ("sys/block/zram0/size", "100\n"),
            ("sys/block/loop3/size", "0\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let n = &v[0];
        assert_eq!(n.id, "storage:nvme0n1");
        assert_eq!(n.title, "SAMSUNG TEST");
        assert_eq!(spec(n, "Capacity").display(), "1.0 TB");
        assert_eq!(spec(n, "Interface").display(), "NVMe");
        assert_eq!(spec(n, "Type").display(), "NVMe solid-state drive");
        assert!(spec(n, "Serial number").identifier);
        assert_eq!(n.subtitle.as_deref(), Some("1.0 TB · NVMe solid-state drive"));
    }

    #[test]
    fn sata_hdd_and_usb_detected_from_symlink_path() {
        let fx = Fx::new(&[
            ("sys/block/sda/device/model", "WDC WD10\n"),
            ("sys/block/sda/device/rev", "01.0\n"),
            ("sys/block/sda/size", "1953525168\n"),
            ("sys/block/sda/queue/rotational", "1\n"),
            ("sys/block/sdb/size", "62552064\n"),
            ("sys/block/sdb/queue/rotational", "0\n"),
        ]);
        // Replace the directories' parents with symlinks is impractical; use link files for transport:
        let fx2 = Fx::new(&[]);
        fx2.symlink("sys/block/sda", "../devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda");
        fx2.symlink("sys/block/sdb", "../devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host6/target6:0:0/6:0:0:0/block/sdb");
        assert_eq!(transport(fx2.path(), "sda"), Some("SATA"));
        assert_eq!(transport(fx2.path(), "sdb"), Some("USB"));
        let v = collect_from(fx.path());
        assert_eq!(spec(&v[0], "Type").display(), "Hard disk drive");
        assert_eq!(spec(&v[0], "Firmware").display(), "01.0");
        assert_eq!(spec(&v[0], "Interface").display(), "Not reported");
    }

    #[test]
    fn malformed_and_missing_values_degrade_without_panicking() {
        let fx = Fx::new(&[("sys/block/sda/size", "abc\n"), ("sys/block/sda/removable", "7\n")]);
        let v = collect_from(fx.path());
        let c = &v[0];
        assert_eq!(c.title, "Storage device sda");
        assert!(matches!(spec(c, "Capacity").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Removable").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Model").display(), "Not reported");
    }

    #[test]
    fn unreadable_root_yields_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }
}
