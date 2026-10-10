//! USB devices from `/sys/bus/usb/devices`. Interface entries (`1-2:1.0`) are not devices; they
//! only contribute function classes and bound drivers. Parent is the upstream hub, or for a root
//! hub the PCI host controller.

use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use super::ids::IdDb;
use super::{bus_parent, link_name, opt_spec, parse_hex, read_attr, read_hex, real_components};
use crate::util::{list_dir, parse_field, source, text_spec};

pub(super) fn collect(root: &Path, db: Option<&IdDb>) -> Vec<Component> {
    let names = list_dir(root, "sys/bus/usb/devices");
    names
        .iter()
        .filter(|n| super::is_usb_device_name(n))
        .map(|n| read_device(root, n, &names, db))
        .collect()
}

/// "2.10" -> "USB 2.1". bcdUSB is reported as major.minor-with-two-digits.
pub(crate) fn version_label(v: &str) -> Option<String> {
    let (maj, min) = v.trim().split_once('.')?;
    let maj: u32 = maj.trim().parse().ok()?;
    let m = min.chars().next()?.to_digit(10)?;
    Some(format!("USB {maj}.{m}"))
}

fn parse_speed(s: &String) -> Result<Value, String> {
    let m: f64 = s.trim().parse().map_err(|_| format!("unrecognised speed {s:?}"))?;
    if m.is_finite() && m > 0.0 { Ok(Value::Bps((m * 1e6).round() as u64)) } else { Err(format!("unrecognised speed {s:?}")) }
}

fn parse_power(s: &String) -> Result<Value, String> {
    let digits = s.trim().trim_end_matches("mA").trim();
    digits.parse::<u32>().map(|n| Value::Text(format!("{n} mA"))).map_err(|_| format!("unrecognised power {s:?}"))
}

fn parent_id(root: &Path, name: &str) -> Option<String> {
    let comps = real_components(root, &format!("sys/bus/usb/devices/{name}"));
    if let Some((_, up)) = comps.split_last()
        && let Some(p) = bus_parent(up)
    {
        return Some(p);
    }
    // Path unresolvable (flattened tree): derive from the port path.
    match name.rsplit_once('.') {
        Some((p, _)) => Some(format!("usb:{p}")),
        None => name.split_once('-').map(|(bus, _)| format!("usb:usb{bus}")).filter(|_| !name.starts_with("usb")),
    }
}

fn read_device(root: &Path, name: &str, all: &[String], db: Option<&IdDb>) -> Component {
    let base = format!("sys/bus/usb/devices/{name}");
    let attr = |a: &str| read_attr(root, &format!("{base}/{a}"));
    let is_root_hub = name.starts_with("usb");

    let vid_f = read_hex(root, &format!("{base}/idVendor"));
    let pid_f = read_hex(root, &format!("{base}/idProduct"));
    let (vid, pid) = (vid_f.value().map(|v| *v as u16), pid_f.value().map(|v| *v as u16));
    let manufacturer = attr("manufacturer");
    let product = attr("product");
    let serial = attr("serial");
    let class_f = attr("bDeviceClass");
    let class = class_f.value().and_then(|s| parse_hex(s).ok()).map(|c| c as u8);
    let version_f = attr("version");
    let speed_f = parse_field(attr("speed"), parse_speed);
    let power_f = parse_field(attr("bMaxPower"), parse_power);

    let vendor_db = vid.and_then(|v| db?.vendor(v)).map(str::to_owned);
    let device_db = vid.zip(pid).and_then(|(v, p)| db?.device(v, p)).map(str::to_owned);
    let vendor_display = vendor_db.clone().or_else(|| manufacturer.value().cloned());
    let ver_label = version_f.value().and_then(|v| version_label(v));

    // Interface-level functions and drivers.
    let prefix = format!("{name}:");
    let mut fn_classes: Vec<String> = Vec::new();
    let mut drivers: Vec<String> = Vec::new();
    for iface in all.iter().filter(|n| n.starts_with(&prefix)) {
        let ib = format!("sys/bus/usb/devices/{iface}");
        if let Some(c) = read_hex(root, &format!("{ib}/bInterfaceClass")).value() {
            let label = db.and_then(|d| d.class(*c as u8)).map(str::to_owned).unwrap_or_else(|| format!("Class 0x{:02x}", c));
            if !fn_classes.contains(&label) {
                fn_classes.push(label);
            }
        }
        if let Some(d) = link_name(root, &format!("{ib}/driver"))
            && !drivers.contains(&d)
        {
            drivers.push(d);
        }
    }
    let device_driver = link_name(root, &format!("{base}/driver"));
    let driver = if drivers.is_empty() { device_driver.clone() } else { Some(drivers.join(", ")) };

    let class_name = class.and_then(|c| match c {
        0x00 => Some("Defined by each function".to_owned()),
        c => db.and_then(|d| d.class(c)).map(str::to_owned).or_else(|| Some(format!("Class 0x{c:02x}"))),
    });
    let type_text = match class {
        Some(0x00) | Some(0xef) if !fn_classes.is_empty() => Some(fn_classes.join(", ")),
        _ => class_name.clone(),
    };

    let title = if is_root_hub {
        format!("{} root hub ({name})", ver_label.clone().unwrap_or_else(|| "USB".into()))
    } else {
        product.value().cloned().or(device_db.clone()).unwrap_or_else(|| match (vid, pid) {
            (Some(v), Some(p)) => format!("USB device {v:04x}:{p:04x}"),
            _ => format!("USB device {name}"),
        })
    };
    let subtitle = [ver_label.clone(), speed_f.value().map(Value::display), vendor_display.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");

    let busnum = attr("busnum");
    let devnum = attr("devnum");
    let ids_src = |a: &str| match &vendor_db {
        Some(_) => format!("{} + {}", source(&format!("{base}/{a}")), db.map(|d| d.path.as_str()).unwrap_or("")),
        None => source(&format!("{base}/{a}")),
    };
    let hex4 = |f: &Field<u32>| parse_field(f.clone(), |n| Ok(Value::Text(format!("0x{n:04x}"))));

    let mut c = Component::new(&format!("usb:{name}"), Category::Peripherals, &title);
    if !subtitle.is_empty() {
        c = c.subtitle(&subtitle);
    }
    if let Some(p) = parent_id(root, name) {
        c = c.parent(&p);
    }
    c.summary(vec![
        Spec::new("Speed", speed_f.clone()),
        opt_spec("Type", type_text, &source(&format!("{base}/bDeviceClass"))),
        match vendor_display {
            Some(v) => Spec::text("Vendor", v, &ids_src("idVendor")),
            None => Spec::new("Vendor", parse_field(vid_f.clone(), |n| Ok(Value::Text(format!("0x{n:04x} (not in database)"))))),
        },
    ])
    .group(SpecGroup::new("Identity", vec![
        match &vendor_db {
            Some(v) => Spec::text("Vendor", v.clone(), &ids_src("idVendor")),
            None => Spec::new("Vendor", parse_field(vid_f.clone(), |n| Ok(Value::Text(format!("0x{n:04x} (not in database)"))))),
        },
        text_spec("Manufacturer string", manufacturer),
        text_spec("Product string", product),
        opt_spec("Database name", device_db, &ids_src("idProduct")),
        text_spec("Serial number", serial).identifier(),
    ]))
    .group(SpecGroup::new("Connection", vec![
        opt_spec("USB version", ver_label, &source(&format!("{base}/version"))),
        Spec::new("Negotiated speed", speed_f).live(),
        opt_spec("Device class", class_name, &source(&format!("{base}/bDeviceClass"))),
        opt_spec("Functions", if fn_classes.is_empty() { None } else { Some(fn_classes.join(", ")) }, &source(&format!("{base}:*/bInterfaceClass"))),
        opt_spec("Driver", driver, &source(&format!("{base}/driver"))),
        text_spec("Removable", attr("removable")),
        Spec::text("Port path", name, &source(&base)),
    ]))
    .group(SpecGroup::new("Power", vec![
        Spec::new("Maximum power draw", power_f).rated(),
        text_spec("Runtime power state", attr("power/runtime_status")).live(),
    ]))
    .group(SpecGroup::new("Raw identifiers", vec![
        Spec::new("Vendor ID", hex4(&vid_f)),
        Spec::new("Product ID", hex4(&pid_f)),
        text_spec("Device release (bcdDevice)", attr("bcdDevice")),
        text_spec("bcdUSB", version_f),
        text_spec("Bus number", busnum),
        text_spec("Device number", devnum),
        text_spec("bDeviceClass", class_f),
    ]).technical())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    const USB_IDS: &str = "046d  Logitech, Inc.\n\tc52b  Unifying Receiver\n1d6b  Linux Foundation\n\t0002  2.0 root hub\n\t0003  3.0 root hub\n05e3  Genesys Logic, Inc.\n\t0610  Hub\nC 00  (Defined at Interface level)\nC 03  Human Interface Device\nC 09  Hub\nC 0e  Video\nC ef  Miscellaneous Device\n";

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    #[allow(clippy::too_many_arguments)]
    fn add_dev(fx: &Fx, path: &str, name: &str, vid: &str, pid: &str, class: &str, speed: &str, ver: &str, product: &str) {
        let real = format!("sys/devices/{path}");
        for (f, v) in [("idVendor", vid), ("idProduct", pid), ("bDeviceClass", class), ("speed", speed), ("version", ver), ("product", product), ("busnum", "1"), ("devnum", "2"), ("bMaxPower", "100mA"), ("bcdDevice", "0100")] {
            fx.write(&format!("{real}/{f}"), &format!("{v}\n"));
        }
        fx.symlink(&format!("sys/bus/usb/devices/{name}"), &format!("../../../devices/{path}"));
    }

    fn run(fx: &Fx) -> Vec<Component> {
        collect(fx.path(), IdDb::usb(fx.path()).as_ref())
    }

    fn dock_tree() -> Fx {
        let fx = Fx::new(&[("usr/share/hwdata/usb.ids", USB_IDS)]);
        let pci = "pci0000:00/0000:00:14.0";
        add_dev(&fx, &format!("{pci}/usb1"), "usb1", "0x1d6b", "0x0002", "09", "480", " 2.00", "xHCI Host Controller");
        add_dev(&fx, &format!("{pci}/usb1/1-2"), "1-2", "0x05e3", "0x0610", "09", "480", " 2.10", "USB2.1 Hub");
        add_dev(&fx, &format!("{pci}/usb1/1-2/1-2.3"), "1-2.3", "0x046d", "0xc52b", "00", "12", " 2.00", "USB Receiver");
        fx.write(&format!("sys/devices/{pci}/usb1/1-2/1-2.3/serial"), "A1B2\n");
        fx.write(&format!("sys/devices/{pci}/usb1/1-2/1-2.3/manufacturer"), "Logitech\n");
        // Interfaces: entries that must not become devices.
        fx.write(&format!("sys/devices/{pci}/usb1/1-2/1-2.3/1-2.3:1.0/bInterfaceClass"), "03\n");
        fx.symlink("sys/bus/usb/devices/1-2.3:1.0", &format!("../../../devices/{pci}/usb1/1-2/1-2.3/1-2.3:1.0"));
        fx.symlink(&format!("sys/devices/{pci}/usb1/1-2/1-2.3/1-2.3:1.0/driver"), "../../../../../../../../../bus/usb/drivers/usbhid");
        fx
    }

    #[test]
    fn device_behind_dock_hub_forms_a_tree() {
        let fx = dock_tree();
        let v = run(&fx);
        assert_eq!(v.len(), 3, "interfaces are not devices");
        let get = |id: &str| v.iter().find(|c| c.id == id).unwrap();
        let root_hub = get("usb:usb1");
        assert_eq!(root_hub.parent.as_deref(), Some("pci:0000:00:14.0"));
        assert_eq!(root_hub.title, "USB 2.0 root hub (usb1)");
        let hub = get("usb:1-2");
        assert_eq!(hub.parent.as_deref(), Some("usb:usb1"));
        let rx = get("usb:1-2.3");
        assert_eq!(rx.parent.as_deref(), Some("usb:1-2"));
        assert_eq!(rx.title, "USB Receiver");
        assert_eq!(rx.subtitle.as_deref(), Some("USB 2.0 · 12 Mb/s · Logitech, Inc."));
        assert_eq!(spec(rx, "Type").display(), "Human Interface Device");
        assert_eq!(spec(rx, "Driver").display(), "usbhid");
        assert!(spec(rx, "Serial number").identifier);
        assert_eq!(spec(rx, "Serial number").display(), "A1B2");
        assert_eq!(spec(rx, "Maximum power draw").display(), "100 mA");
        assert!(spec(rx, "Maximum power draw").rated);
        assert_eq!(spec(hub, "Type").display(), "Hub");
    }

    #[test]
    fn no_ids_database_uses_hex_and_strings() {
        let fx = Fx::new(&[]);
        add_dev(&fx, "pci0000:00/0000:00:14.0/usb1/1-6", "1-6", "0x048d", "0xc195", "00", "12", " 2.00", "ITE Device(8258)");
        let v = run(&fx);
        assert_eq!(v[0].title, "ITE Device(8258)");
        assert_eq!(spec(&v[0], "Vendor").display(), "0x048d (not in database)");
        assert_eq!(v[0].subtitle.as_deref(), Some("USB 2.0 · 12 Mb/s"));
        assert_eq!(v[0].parent.as_deref(), Some("usb:usb1"));
    }

    #[test]
    fn interfaces_only_and_empty_directories() {
        let fx = Fx::new(&[("sys/bus/usb/devices/1-2:1.0/bInterfaceClass", "03\n"), ("sys/bus/usb/devices/2-0:1.0/x", "")]);
        assert!(run(&fx).is_empty());
        let fx = Fx::new(&[]);
        std::fs::create_dir_all(fx.path().join("sys/bus/usb/devices")).unwrap();
        assert!(run(&fx).is_empty());
    }

    #[test]
    fn flat_tree_derives_parent_from_port_path() {
        let fx = Fx::new(&[
            ("sys/bus/usb/devices/1-4.2/idVendor", "046d\n"),
            ("sys/bus/usb/devices/1-4.2/speed", "5000\n"),
            ("sys/bus/usb/devices/1-4.2/version", " 3.20\n"),
            ("sys/bus/usb/devices/2-1/speed", "unknown\n"),
        ]);
        let v = run(&fx);
        let a = v.iter().find(|c| c.id == "usb:1-4.2").unwrap();
        assert_eq!(a.parent.as_deref(), Some("usb:1-4"));
        assert_eq!(a.subtitle.as_deref(), Some("USB 3.2 · 5 Gb/s"));
        assert_eq!(a.title, "USB device 1-4.2");
        let b = v.iter().find(|c| c.id == "usb:2-1").unwrap();
        assert_eq!(b.parent.as_deref(), Some("usb:usb2"));
        assert!(matches!(spec(b, "Negotiated speed").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(b, "Maximum power draw").display(), "Not reported");
    }

    #[test]
    fn non_utf8_strings_survive() {
        let fx = Fx::new(&[]);
        fx.write_bytes("sys/bus/usb/devices/1-1/product", b"Cam\xff\xfe era\n");
        fx.write_bytes("sys/bus/usb/devices/1-1/serial", b"\xff\xff\n");
        let v = run(&fx);
        assert!(v[0].title.starts_with("Cam"));
        assert!(spec(&v[0], "Serial number").is_available());
    }

    #[test]
    fn version_labels() {
        assert_eq!(version_label(" 2.10").as_deref(), Some("USB 2.1"));
        assert_eq!(version_label("3.00").as_deref(), Some("USB 3.0"));
        assert_eq!(version_label("garbage"), None);
        assert_eq!(version_label("").as_deref(), None);
    }
}
