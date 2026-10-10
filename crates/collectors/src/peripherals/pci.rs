//! PCI functions from `/sys/bus/pci/devices`. Display-class functions (0x03xxxx) are skipped
//! entirely and bridges (0x06xxxx) are not listed. Link attributes are read only when the function
//! reports an active runtime-PM state; everything else read here is cached by the kernel.

use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use super::ids::IdDb;
use super::{is_pci_addr, link_name, pci_is_awake, pci_runtime_status, read_attr, read_hex};
use crate::util::{list_dir, parse_field, source, text_spec};

/// Display controllers belong to the graphics collector; bridges are plumbing, not devices.
pub(crate) fn is_listed_class(class: u32) -> bool {
    !matches!((class >> 16) & 0xff, 0x03 | 0x06)
}

/// Whether `pci:<addr>` is emitted by [`collect`]. Reads only the cached `class` attribute.
pub(crate) fn is_listed(root: &Path, addr: &str) -> bool {
    is_pci_addr(addr) && read_hex(root, &format!("sys/bus/pci/devices/{addr}/class")).value().is_some_and(|c| is_listed_class(*c))
}

pub(super) fn collect(root: &Path, db: Option<&IdDb>) -> Vec<Component> {
    list_dir(root, "sys/bus/pci/devices").into_iter().filter(|a| is_pci_addr(a)).filter_map(|a| read_device(root, &a, db)).collect()
}

pub(crate) fn name_spec(label: &str, id: &Field<u32>, name: Option<&str>, attr_src: &str, db: Option<&IdDb>, width: usize) -> Spec {
    let src = match (name, db) {
        (Some(_), Some(d)) => format!("{attr_src} + {}", d.path),
        _ => attr_src.to_owned(),
    };
    let f = parse_field(id.clone(), |n| {
        Ok(Value::Text(match name {
            Some(t) => t.to_owned(),
            None => format!("{n:0width$x}"),
        }))
    });
    Spec::new(label, Field { availability: f.availability, source: src })
}

fn read_device(root: &Path, addr: &str, db: Option<&IdDb>) -> Option<Component> {
    let base = format!("sys/bus/pci/devices/{addr}");
    // Without a readable class we cannot prove this is not a display function, so skip it.
    let class_f = read_hex(root, &format!("{base}/class"));
    let class = *class_f.value()?;
    if !is_listed_class(class) {
        return None;
    }
    let vendor_f = read_hex(root, &format!("{base}/vendor"));
    let device_f = read_hex(root, &format!("{base}/device"));
    let sv_f = read_hex(root, &format!("{base}/subsystem_vendor"));
    let sd_f = read_hex(root, &format!("{base}/subsystem_device"));
    let vid = vendor_f.value().map(|v| *v as u16);
    let did = device_f.value().map(|v| *v as u16);

    let vendor_name = vid.and_then(|v| db?.vendor(v)).map(str::to_owned);
    let device_name = vid.zip(did).and_then(|(v, d)| db?.device(v, d)).map(str::to_owned);
    let (cb, sb) = ((class >> 16) as u8, (class >> 8) as u8);
    let class_name = db.and_then(|d| d.class(cb)).map(str::to_owned);
    let sub_name = db.and_then(|d| d.subclass(cb, sb)).map(str::to_owned);
    let class_label = sub_name.clone().or_else(|| class_name.clone()).unwrap_or_else(|| format!("Class {:04x}", class >> 8));

    let title = match (&device_name, vid, did) {
        (Some(n), _, _) => n.clone(),
        (None, Some(v), Some(d)) => match &vendor_name {
            Some(vn) => format!("{vn} device {d:04x}"),
            None => format!("PCI device {v:04x}:{d:04x}"),
        },
        _ => format!("PCI device {addr}"),
    };

    let driver = link_name(root, &format!("{base}/driver"));
    let driver_src = source(&format!("{base}/driver"));
    let status = pci_runtime_status(root, addr);
    let awake = pci_is_awake(status.as_deref());

    let link = |attr: &str| -> Field<String> {
        let rel = format!("{base}/{attr}");
        if !awake {
            return Field::not_reported(source(&rel));
        }
        // "Unknown speed" / width 0 mean the function has no PCIe link (integrated devices).
        match read_attr(root, &rel) {
            f if f.value().is_some_and(|v| v.starts_with("Unknown") || v == "0") => Field::not_reported(f.source),
            f => f,
        }
    };
    let cur_speed = link("current_link_speed");
    let cur_width = link("current_link_width");
    let max_speed = link("max_link_speed");
    let max_width = link("max_link_width");
    let width_val = |f: Field<String>| parse_field(f, |s| s.parse::<u32>().map(|n| Value::Text(format!("x{n}"))).map_err(|e| e.to_string()));

    let current_link = match (cur_speed.value(), cur_width.value()) {
        (Some(s), Some(w)) => Some(format!("{s} x{w}")),
        _ => None,
    };
    let link_src = source(&format!("{base}/current_link_speed"));
    let iommu = link_name(root, &format!("{base}/iommu_group"));

    let sub_vendor_name = sv_f.value().and_then(|v| db?.vendor(*v as u16)).map(str::to_owned);
    let subsystem = match (sv_f.value(), sd_f.value()) {
        (Some(v), Some(d)) if *v != 0 || *d != 0 => Some(format!("{}{:04x}:{:04x}", sub_vendor_name.map(|n| format!("{n} ")).unwrap_or_default(), v, d)),
        _ => None,
    };

    let mut c = Component::new(&format!("pci:{addr}"), Category::Peripherals, &title);
    c = c.subtitle(&[Some(class_label.clone()), vendor_name.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · "));
    c = c.summary(vec![
        Spec::text("Type", class_label, &source(&format!("{base}/class"))),
        super::opt_spec("Driver", driver.clone(), &driver_src),
        match current_link.clone() {
            Some(l) => Spec::text("Link", l, &link_src).live(),
            None => Spec::missing("Link", &link_src).live(),
        },
    ]);
    c = c.group(SpecGroup::new("Identity", vec![
        name_spec("Vendor", &vendor_f, vendor_name.as_deref(), &source(&format!("{base}/vendor")), db, 4),
        name_spec("Device", &device_f, device_name.as_deref(), &source(&format!("{base}/device")), db, 4),
        super::opt_spec("Class", class_name, &source(&format!("{base}/class"))),
        super::opt_spec("Subsystem", subsystem, &source(&format!("{base}/subsystem_vendor"))),
    ]));
    c = c.group(SpecGroup::new("Connection", vec![
        Spec::text("Bus address", addr, &source(&base)),
        super::opt_spec("Driver", driver, &driver_src),
        text_spec("Current link speed", cur_speed).live(),
        Spec::new("Current link width", width_val(cur_width)).live(),
        text_spec("Maximum link speed", max_speed).rated(),
        Spec::new("Maximum link width", width_val(max_width)).rated(),
        Spec::new("IOMMU group", match iommu {
            Some(g) => parse_field(Field::available(g, source(&format!("{base}/iommu_group"))), |s| s.parse::<u64>().map(Value::Count).map_err(|e| e.to_string())),
            None => Field::not_reported(source(&format!("{base}/iommu_group"))),
        }),
    ]));
    c = c.group(SpecGroup::new("Power", vec![
        match &status {
            Some(s) => Spec::text("Runtime power state", s.clone(), &source(&format!("{base}/power/runtime_status"))).live(),
            None => Spec::missing("Runtime power state", &source(&format!("{base}/power/runtime_status"))),
        },
        Spec::new("Link details read", Field::available(Value::Bool(awake), source(&format!("{base}/power/runtime_status")))),
    ]));
    c = c.group(SpecGroup::new("Raw identifiers", vec![
        Spec::new("Vendor ID", parse_field(vendor_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
        Spec::new("Device ID", parse_field(device_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
        Spec::new("Class code", parse_field(class_f, |n| Ok(Value::Text(format!("0x{n:06x}"))))),
        Spec::new("Subsystem vendor ID", parse_field(sv_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
        Spec::new("Subsystem device ID", parse_field(sd_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
    ]).technical());
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    const IDS: &str = "14c3  MediaTek Inc.\n\t7925  MT7925 Wi-Fi 7\n8086  Intel Corporation\n\t7ae0  Alder Lake-S PCH USB 3.2 xHCI\nC 02  Network controller\n\t80  Network controller\nC 0c  Serial bus controller\n\t03  USB controller\n";

    pub fn add_pci(fx: &Fx, addr: &str, class: &str, ven: &str, dev: &str, status: &str, driver: Option<&str>) {
        let b = format!("sys/bus/pci/devices/{addr}");
        fx.write(&format!("{b}/class"), &format!("{class}\n"));
        fx.write(&format!("{b}/vendor"), &format!("{ven}\n"));
        fx.write(&format!("{b}/device"), &format!("{dev}\n"));
        fx.write(&format!("{b}/subsystem_vendor"), "0x17aa\n");
        fx.write(&format!("{b}/subsystem_device"), "0xe0ff\n");
        fx.write(&format!("{b}/power/runtime_status"), &format!("{status}\n"));
        fx.write(&format!("{b}/current_link_speed"), "5.0 GT/s PCIe\n");
        fx.write(&format!("{b}/max_link_speed"), "8.0 GT/s PCIe\n");
        fx.write(&format!("{b}/current_link_width"), "1\n");
        fx.write(&format!("{b}/max_link_width"), "2\n");
        if let Some(d) = driver {
            fx.symlink(&format!("{b}/driver"), &format!("../../../../bus/pci/drivers/{d}"));
        }
        fx.symlink(&format!("{b}/iommu_group"), "../../../../kernel/iommu_groups/19");
    }

    #[test]
    fn wifi_card_with_ids_database() {
        let fx = Fx::new(&[("usr/share/hwdata/pci.ids", IDS)]);
        add_pci(&fx, "0000:12:00.0", "0x028000", "0x14c3", "0x7925", "active", Some("mt7925e"));
        let v = collect_from_pci(&fx);
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "pci:0000:12:00.0");
        assert_eq!(c.title, "MT7925 Wi-Fi 7");
        assert_eq!(c.subtitle.as_deref(), Some("Network controller · MediaTek Inc."));
        assert_eq!(spec(c, "Vendor").display(), "MediaTek Inc.");
        assert_eq!(spec(c, "Driver").display(), "mt7925e");
        assert_eq!(spec(c, "Link").display(), "5.0 GT/s PCIe x1");
        assert!(spec(c, "Maximum link speed").rated);
        assert!(spec(c, "Current link speed").live);
        assert_eq!(spec(c, "Maximum link width").display(), "x2");
        assert_eq!(spec(c, "IOMMU group").display(), "19");
    }

    fn collect_from_pci(fx: &Fx) -> Vec<Component> {
        collect(fx.path(), IdDb::pci(fx.path()).as_ref())
    }

    #[test]
    fn no_ids_database_falls_back_to_hex() {
        let fx = Fx::new(&[]);
        add_pci(&fx, "0000:00:14.0", "0x0c0330", "0x8086", "0x7ae0", "active", None);
        let c = &collect_from_pci(&fx)[0];
        assert_eq!(c.title, "PCI device 8086:7ae0");
        assert_eq!(spec(c, "Vendor").display(), "8086");
        assert_eq!(spec(c, "Type").display(), "Class 0c03");
        assert_eq!(spec(c, "Driver").display(), "Not reported");
    }

    #[test]
    fn display_and_bridges_are_skipped_and_suspended_links_not_read() {
        let fx = Fx::new(&[("usr/share/hwdata/pci.ids", IDS)]);
        add_pci(&fx, "0000:01:00.0", "0x030000", "0x10de", "0x2d58", "suspended", Some("nvidia"));
        add_pci(&fx, "0000:00:01.0", "0x060400", "0x8086", "0x7ae0", "active", None);
        add_pci(&fx, "0000:01:00.1", "0x040300", "0x10de", "0x22eb", "suspended", Some("snd_hda_intel"));
        // A trap: any read of the display function's link files would be a failure in review.
        let v = collect_from_pci(&fx);
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "pci:0000:01:00.1");
        assert!(matches!(spec(c, "Current link speed").value.availability, Availability::NotReported));
        assert!(matches!(spec(c, "Maximum link speed").value.availability, Availability::NotReported));
        assert_eq!(spec(c, "Runtime power state").display(), "suspended");
        assert_eq!(spec(c, "Driver").display(), "snd_hda_intel");
        assert!(!is_listed(fx.path(), "0000:01:00.0"));
        assert!(is_listed(fx.path(), "0000:01:00.1"));
    }

    #[test]
    fn malformed_values_degrade() {
        let fx = Fx::new(&[]);
        add_pci(&fx, "0000:05:00.0", "0x010802", "zzzz", "0x1234", "active", None);
        fx.write("sys/bus/pci/devices/0000:05:00.0/current_link_width", "wide\n");
        let c = &collect_from_pci(&fx)[0];
        assert!(matches!(spec(c, "Vendor ID").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Current link width").value.availability, Availability::Failed { .. }));
        assert_eq!(c.title, "PCI device 0000:05:00.0");
    }

    #[test]
    fn missing_class_or_status_is_conservative() {
        let fx = Fx::new(&[("sys/bus/pci/devices/0000:09:00.0/vendor", "0x1234\n")]);
        assert!(collect_from_pci(&fx).is_empty(), "cannot classify, so skipped");
        let fx = Fx::new(&[
            ("sys/bus/pci/devices/0000:09:00.0/class", "0x020000\n"),
            ("sys/bus/pci/devices/0000:09:00.0/current_link_speed", "2.5 GT/s PCIe\n"),
        ]);
        let c = &collect_from_pci(&fx)[0];
        assert!(matches!(spec(c, "Current link speed").value.availability, Availability::NotReported), "unverifiable power state: not read");
    }

    #[test]
    fn vm_virtio_style_devices() {
        let fx = Fx::new(&[]);
        add_pci(&fx, "0000:00:03.0", "0x020000", "0x1af4", "0x1000", "unsupported", Some("virtio-pci"));
        let c = &collect_from_pci(&fx)[0];
        assert_eq!(c.title, "PCI device 1af4:1000");
        assert_eq!(spec(c, "Link").display(), "5.0 GT/s PCIe x1");
    }
}
