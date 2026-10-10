//! Physical network adapters from `/sys/class/net`. Interfaces without a `device` link (loopback,
//! bridges, veth, tunnels, VPNs) are not hardware and are skipped. Wi-Fi capabilities that need
//! nl80211 are reported as not available rather than guessed; no sockets are opened.

use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::peripherals::ids::IdDb;
use crate::peripherals::{bus_parent, link_name, opt_spec, pci, read_attr, read_hex, real_components};
use crate::util::{list_dir, parse_field, source, text_spec};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let pci_db = IdDb::pci(root);
    let usb_db = IdDb::usb(root);
    list_dir(root, "sys/class/net")
        .into_iter()
        .filter(|n| crate::util::exists(root, &format!("sys/class/net/{n}/device")))
        .map(|n| read_adapter(root, &n, pci_db.as_ref(), usb_db.as_ref()))
        .collect()
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Ethernet,
    WiFi,
    Cellular,
    Other,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Ethernet => "Ethernet",
            Kind::WiFi => "Wi-Fi",
            Kind::Cellular => "Cellular",
            Kind::Other => "Other",
        }
    }
}

fn uevent_value(root: &Path, base: &str, key: &str) -> Option<String> {
    let f = read_attr(root, &format!("{base}/uevent"));
    f.value()?.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=').map(str::to_owned))
}

fn kind_of(root: &Path, name: &str, base: &str, driver: Option<&str>) -> Kind {
    if crate::util::exists(root, &format!("{base}/wireless")) || crate::util::exists(root, &format!("{base}/phy80211")) {
        return Kind::WiFi;
    }
    let devtype = uevent_value(root, base, "DEVTYPE");
    if devtype.as_deref() == Some("wwan") || name.starts_with("ww") || matches!(driver, Some("qmi_wwan") | Some("cdc_mbim") | Some("rmnet")) {
        return Kind::Cellular;
    }
    if devtype.as_deref() == Some("wlan") {
        return Kind::WiFi;
    }
    match read_attr(root, &format!("{base}/type")).value().map(String::as_str) {
        Some("1") => Kind::Ethernet,
        _ => Kind::Other,
    }
}

fn state_word(s: &str) -> String {
    let t = s.replace(['-', '_'], " ");
    let mut c = t.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn parse_mbps(s: &String) -> Result<Value, String> {
    let n: i64 = s.trim().parse().map_err(|_| format!("unrecognised speed {s:?}"))?;
    // -1 (and 0) mean the driver cannot tell; that is "unknown", never 0 Mb/s.
    if n > 0 { Ok(Value::Bps(n as u64 * 1_000_000)) } else { Err(String::new()) }
}

/// Like `parse_field`, but an "unknown" marker becomes NotReported instead of Failed.
fn unknown_is_missing<T>(f: Field<T>) -> Field<T> {
    match f.availability {
        facet_core::Availability::Failed { ref reason } if reason.is_empty() => Field::not_reported(f.source),
        _ => f,
    }
}

fn read_adapter(root: &Path, name: &str, pci_db: Option<&IdDb>, usb_db: Option<&IdDb>) -> Component {
    let base = format!("sys/class/net/{name}");
    let dev = format!("{base}/device");
    let driver = link_name(root, &format!("{dev}/driver"));
    let subsystem = link_name(root, &format!("{dev}/subsystem"));
    let kind = kind_of(root, name, &base, driver.as_deref());

    // Resolve the bus location from the real path of the device link.
    let comps = real_components(root, &dev);
    let parent = bus_parent(&comps).filter(|p| match p.strip_prefix("pci:") {
        Some(a) => pci::is_listed(root, a),
        None => true,
    });
    let (bus_name, address) = match parent.as_deref().and_then(|p| p.split_once(':')) {
        Some(("pci", a)) => (Some(if subsystem.as_deref() == Some("virtio") { "VirtIO (PCI)" } else { "PCI" }.to_owned()), Some(a.to_owned())),
        Some(("usb", a)) => (Some("USB".to_owned()), Some(a.to_owned())),
        _ => (subsystem.map(|s| state_word(&s)), None),
    };

    // Vendor and device names from the parent function.
    let (vendor_f, device_f, vendor_name, device_name) = match parent.as_deref().and_then(|p| p.split_once(':')) {
        Some(("pci", a)) => {
            let b = format!("sys/bus/pci/devices/{a}");
            let (v, d) = (read_hex(root, &format!("{b}/vendor")), read_hex(root, &format!("{b}/device")));
            let (vn, dn) = match (v.value(), d.value()) {
                (Some(v), Some(d)) => (pci_db.and_then(|db| db.vendor(*v as u16)).map(str::to_owned), pci_db.and_then(|db| db.device(*v as u16, *d as u16)).map(str::to_owned)),
                (Some(v), None) => (pci_db.and_then(|db| db.vendor(*v as u16)).map(str::to_owned), None),
                _ => (None, None),
            };
            (v, d, vn, dn)
        }
        Some(("usb", a)) => {
            let b = format!("sys/bus/usb/devices/{a}");
            let (v, d) = (read_hex(root, &format!("{b}/idVendor")), read_hex(root, &format!("{b}/idProduct")));
            let (vn, dn) = match (v.value(), d.value()) {
                (Some(v), Some(d)) => (usb_db.and_then(|db| db.vendor(*v as u16)).map(str::to_owned), usb_db.and_then(|db| db.device(*v as u16, *d as u16)).map(str::to_owned)),
                (Some(v), None) => (usb_db.and_then(|db| db.vendor(*v as u16)).map(str::to_owned), None),
                _ => (None, None),
            };
            let product = read_attr(root, &format!("{b}/product")).value().cloned();
            (v, d, vn, dn.or(product))
        }
        _ => (Field::not_reported(source(&dev)), Field::not_reported(source(&dev)), None, None),
    };

    let operstate = read_attr(root, &format!("{base}/operstate"));
    let carrier = unknown_is_missing(parse_field(read_attr(root, &format!("{base}/carrier")), |s| match s.as_str() {
        "1" => Ok(Value::Bool(true)),
        "0" => Ok(Value::Bool(false)),
        o => Err(format!("unexpected carrier value {o:?}")),
    }));
    let speed = unknown_is_missing(parse_field(read_attr(root, &format!("{base}/speed")), parse_mbps));
    let duplex = {
        let f = read_attr(root, &format!("{base}/duplex"));
        match f.value().map(String::as_str) {
            Some("unknown") => Field::not_reported(f.source),
            _ => f,
        }
    };
    let mtu = parse_field(read_attr(root, &format!("{base}/mtu")), |s| s.parse::<u64>().map(Value::Count).map_err(|e| e.to_string()));
    let mac = read_attr(root, &format!("{base}/address"));

    let title = device_name.clone().unwrap_or_else(|| format!("{} adapter ({name})", kind.label()));
    let mut subtitle: Vec<String> = vec![kind.label().to_owned()];
    if let Some(s) = operstate.value() {
        subtitle.push(state_word(s));
    }
    if let Some(s) = speed.value() {
        subtitle.push(s.display());
    }
    let mut c = Component::new(&format!("network:{name}"), Category::Network, &title).subtitle(&subtitle.join(" · "));
    if let Some(p) = &parent {
        c = c.parent(p);
    }

    let vendor_display = match (&vendor_name, vendor_f.value()) {
        (Some(n), _) => Spec::text("Vendor", n.clone(), &source(&format!("{dev}/vendor"))),
        (None, Some(v)) => Spec::text("Vendor", format!("0x{v:04x} (not in database)"), &source(&format!("{dev}/vendor"))),
        (None, None) => Spec::new("Vendor", parse_field(vendor_f.clone(), |_| Ok(Value::Text(String::new())))),
    };

    let mut conn = vec![
        Spec::new("Link state", parse_field(operstate.clone(), |s| Ok(Value::Text(state_word(s))))).live(),
        Spec::new("Carrier", carrier).live(),
        Spec::new("Negotiated speed", speed.clone()).live(),
        text_spec("Duplex", duplex).live(),
        Spec::new("MTU", mtu).live(),
    ];
    // Rated capability is only shown when the kernel exposes it; sysfs does not (ethtool does).
    conn.push(Spec::missing("Supported link speeds", &source(&format!("{base}/speed"))).rated());

    let mut identity = vec![
        Spec::text("Interface", name, &source(&base)),
        Spec::text("Type", kind.label(), &source(&base)),
        vendor_display.clone(),
        opt_spec("Device", device_name, &source(&format!("{dev}/device"))),
        text_spec("MAC address", mac).identifier(),
    ];
    let mut groups_tail: Vec<SpecGroup> = Vec::new();
    if kind == Kind::WiFi {
        let phy = read_attr(root, &format!("{base}/phy80211/name"));
        identity.push(text_spec("Wi-Fi radio (phy)", phy));
        let rf = list_dir(root, &format!("{base}/phy80211")).into_iter().find(|e| e.starts_with("rfkill"));
        let radio = match rf {
            Some(r) => {
                let soft = read_attr(root, &format!("{base}/phy80211/{r}/soft"));
                let hard = read_attr(root, &format!("{base}/phy80211/{r}/hard"));
                match (soft.value().map(String::as_str), hard.value().map(String::as_str)) {
                    (_, Some("1")) => Spec::text("Radio", "Blocked by hardware switch", &hard.source),
                    (Some("1"), _) => Spec::text("Radio", "Blocked by software", &soft.source),
                    (Some("0"), Some("0")) => Spec::text("Radio", "On", &soft.source),
                    _ => Spec::missing("Radio", &soft.source),
                }
            }
            None => Spec::missing("Radio", &source(&format!("{base}/phy80211/rfkill*/soft"))),
        };
        groups_tail.push(SpecGroup::new("Wi-Fi", vec![
            radio.live(),
            Spec::missing("Supported bands", &source(&format!("{base}/phy80211"))).rated(),
            Spec::missing("Supported standards", &source(&format!("{base}/phy80211"))).rated(),
        ]));
    }

    let summary_speed = Spec::new("Speed", speed).live();
    c.summary(vec![
        Spec::new("Status", parse_field(operstate, |s| Ok(Value::Text(state_word(s))))).live(),
        summary_speed,
        opt_spec("Driver", driver.clone(), &source(&format!("{dev}/driver"))),
    ])
    .group(SpecGroup::new("Identity", identity))
    .group(SpecGroup::new("Connection", conn))
    .group(SpecGroup::new("Hardware", vec![
        opt_spec("Bus", bus_name, &source(&dev)),
        opt_spec("Bus address", address, &source(&dev)),
        opt_spec("Driver", driver, &source(&format!("{dev}/driver"))),
    ]))
    .groups(groups_tail)
    .group(SpecGroup::new("Raw identifiers", vec![
        Spec::new("Vendor ID", parse_field(vendor_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
        Spec::new("Device ID", parse_field(device_f, |n| Ok(Value::Text(format!("0x{n:04x}"))))),
        text_spec("ARP hardware type", read_attr(root, &format!("{base}/type"))),
    ]).technical())
}

trait Groups {
    fn groups(self, g: Vec<SpecGroup>) -> Self;
}

impl Groups for Component {
    fn groups(mut self, g: Vec<SpecGroup>) -> Self {
        self.groups.extend(g);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    const PCI_IDS: &str = "10ec  Realtek Semiconductor Co., Ltd.\n\t8168  RTL8111/8168/8411 PCI Express Gigabit Ethernet Controller\n14c3  MediaTek Inc.\n\t7925  MT7925 802.11be\n1af4  Red Hat, Inc.\n\t1000  Virtio network device\n";

    /// Adds a PCI function with a net interface below it, as sysfs lays it out.
    #[allow(clippy::too_many_arguments)]
    fn add_pci_nic(fx: &Fx, bridge: &str, addr: &str, name: &str, ven: &str, dev: &str, class: &str, driver: &str) {
        let p = format!("sys/devices/pci0000:00/{bridge}/{addr}");
        fx.write(&format!("{p}/vendor"), &format!("{ven}\n"));
        fx.write(&format!("{p}/device"), &format!("{dev}\n"));
        fx.write(&format!("{p}/class"), &format!("{class}\n"));
        fx.write(&format!("{p}/net/{name}/type"), "1\n");
        fx.symlink(&format!("{p}/driver"), &format!("../../../../bus/pci/drivers/{driver}"));
        fx.symlink(&format!("{p}/net/{name}/device"), "../..");
        fx.symlink(&format!("sys/class/net/{name}"), &format!("../../devices/pci0000:00/{bridge}/{addr}/net/{name}"));
        fx.symlink(&format!("sys/bus/pci/devices/{addr}"), &format!("../../../devices/pci0000:00/{bridge}/{addr}"));
        fx.symlink(&format!("{p}/subsystem"), "../../../../bus/pci");
    }

    fn wired_and_wifi() -> Fx {
        let fx = Fx::new(&[("usr/share/hwdata/pci.ids", PCI_IDS)]);
        add_pci_nic(&fx, "0000:00:1b.5", "0000:0c:00.0", "enp12s0", "0x10ec", "0x8168", "0x020000", "r8169");
        add_pci_nic(&fx, "0000:00:1c.4", "0000:12:00.0", "wlp18s0", "0x14c3", "0x7925", "0x028000", "mt7925e");
        let e = "sys/class/net/enp12s0";
        fx.write(&format!("{e}/operstate"), "down\n");
        fx.write(&format!("{e}/speed"), "-1\n");
        fx.write(&format!("{e}/duplex"), "unknown\n");
        fx.write(&format!("{e}/carrier"), "0\n");
        fx.write(&format!("{e}/mtu"), "1500\n");
        fx.write(&format!("{e}/address"), "aa:bb:cc:00:11:22\n");
        let w = "sys/class/net/wlp18s0";
        fx.write(&format!("{w}/operstate"), "up\n");
        fx.write(&format!("{w}/carrier"), "1\n");
        fx.write(&format!("{w}/mtu"), "1500\n");
        fx.write(&format!("{w}/address"), "de:ad:be:ef:00:01\n");
        fx.write(&format!("{w}/wireless/status"), "0\n");
        fx.write(&format!("{w}/phy80211/name"), "phy0\n");
        fx.write(&format!("{w}/phy80211/rfkill3/soft"), "0\n");
        fx.write(&format!("{w}/phy80211/rfkill3/hard"), "0\n");
        // Virtual interfaces have no device link and must be ignored.
        for v in ["lo", "docker0", "tailscale0", "veth123"] {
            fx.write(&format!("sys/class/net/{v}/operstate"), "unknown\n");
        }
        fx
    }

    #[test]
    fn physical_adapters_only_and_wired_vs_wifi() {
        let fx = wired_and_wifi();
        let v = collect_from(fx.path());
        let ids: Vec<&str> = v.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["network:enp12s0", "network:wlp18s0"]);

        let eth = &v[0];
        assert_eq!(eth.title, "RTL8111/8168/8411 PCI Express Gigabit Ethernet Controller");
        assert_eq!(eth.parent.as_deref(), Some("pci:0000:0c:00.0"));
        assert_eq!(spec(eth, "Type").display(), "Ethernet");
        assert_eq!(spec(eth, "Vendor").display(), "Realtek Semiconductor Co., Ltd.");
        assert_eq!(spec(eth, "Driver").display(), "r8169");
        assert_eq!(spec(eth, "Bus").display(), "PCI");
        assert_eq!(spec(eth, "Bus address").display(), "0000:0c:00.0");
        assert_eq!(spec(eth, "Link state").display(), "Down");
        assert_eq!(spec(eth, "Carrier").display(), "No");
        assert_eq!(spec(eth, "Negotiated speed").display(), "Not reported", "speed -1 is unknown, not 0");
        assert_eq!(spec(eth, "Duplex").display(), "Not reported");
        assert_eq!(spec(eth, "MTU").display(), "1500");
        assert!(spec(eth, "MAC address").identifier);
        assert!(spec(eth, "Negotiated speed").live);
        assert!(spec(eth, "Supported link speeds").rated);
        assert_eq!(eth.subtitle.as_deref(), Some("Ethernet · Down"));
        assert!(!eth.groups.iter().any(|g| g.title == "Wi-Fi"));

        let wifi = &v[1];
        assert_eq!(spec(wifi, "Type").display(), "Wi-Fi");
        assert_eq!(spec(wifi, "Wi-Fi radio (phy)").display(), "phy0");
        assert_eq!(spec(wifi, "Radio").display(), "On");
        assert_eq!(spec(wifi, "Supported bands").display(), "Not reported");
        // speed/duplex files that return EINVAL on a Wi-Fi link are simply absent here.
        assert_eq!(spec(wifi, "Negotiated speed").display(), "Not reported");
        assert_eq!(wifi.subtitle.as_deref(), Some("Wi-Fi · Up"));
    }

    #[test]
    fn negotiated_speed_is_live_and_converted() {
        let fx = wired_and_wifi();
        fx.write("sys/class/net/enp12s0/speed", "2500\n");
        fx.write("sys/class/net/enp12s0/duplex", "full\n");
        fx.write("sys/class/net/enp12s0/operstate", "up\n");
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Negotiated speed").display(), "2.5 Gb/s");
        assert_eq!(spec(c, "Duplex").display(), "full");
        assert_eq!(c.subtitle.as_deref(), Some("Ethernet · Up · 2.5 Gb/s"));
    }

    #[test]
    fn malformed_values_degrade_without_panic() {
        let fx = wired_and_wifi();
        fx.write("sys/class/net/enp12s0/speed", "fast\n");
        fx.write("sys/class/net/enp12s0/mtu", "-\n");
        fx.write("sys/class/net/enp12s0/carrier", "maybe\n");
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Negotiated speed").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "MTU").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Carrier").value.availability, Availability::Failed { .. }));
    }

    #[test]
    fn no_ids_database_reports_hex() {
        let fx = wired_and_wifi();
        std::fs::remove_file(fx.path().join("usr/share/hwdata/pci.ids")).unwrap();
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "Ethernet adapter (enp12s0)");
        assert_eq!(spec(c, "Vendor").display(), "0x10ec (not in database)");
        assert_eq!(spec(c, "Device ID").display(), "0x8168");
    }

    #[test]
    fn virtio_nic_in_a_vm_resolves_its_pci_parent() {
        let fx = Fx::new(&[("usr/share/hwdata/pci.ids", PCI_IDS)]);
        let p = "sys/devices/pci0000:00/0000:00:03.0";
        fx.write(&format!("{p}/vendor"), "0x1af4\n");
        fx.write(&format!("{p}/device"), "0x1000\n");
        fx.write(&format!("{p}/class"), "0x020000\n");
        fx.write(&format!("{p}/virtio0/net/eth0/type"), "1\n");
        fx.write(&format!("{p}/virtio0/net/eth0/operstate"), "up\n");
        fx.write(&format!("{p}/virtio0/net/eth0/speed"), "10000\n");
        fx.symlink(&format!("{p}/virtio0/net/eth0/device"), "../..");
        fx.symlink(&format!("{p}/virtio0/driver"), "../../../../bus/virtio/drivers/virtio_net");
        fx.symlink(&format!("{p}/virtio0/subsystem"), "../../../../bus/virtio");
        fx.symlink("sys/class/net/eth0", "../../devices/pci0000:00/0000:00:03.0/virtio0/net/eth0");
        fx.symlink("sys/bus/pci/devices/0000:00:03.0", "../../../devices/pci0000:00/0000:00:03.0");
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.parent.as_deref(), Some("pci:0000:00:03.0"));
        assert_eq!(c.title, "Virtio network device");
        assert_eq!(spec(c, "Bus").display(), "VirtIO (PCI)");
        assert_eq!(spec(c, "Driver").display(), "virtio_net");
        assert_eq!(spec(c, "Negotiated speed").display(), "10 Gb/s");
    }

    #[test]
    fn usb_ethernet_dongle_parents_to_the_usb_device() {
        let fx = Fx::new(&[("usr/share/hwdata/usb.ids", "0b95  ASIX Electronics Corp.\n\t1790  AX88179 Gigabit Ethernet\n")]);
        let d = "sys/devices/pci0000:00/0000:00:14.0/usb2/2-1";
        fx.write(&format!("{d}/idVendor"), "0b95\n");
        fx.write(&format!("{d}/idProduct"), "1790\n");
        fx.write(&format!("{d}/2-1:1.0/net/enx001122/type"), "1\n");
        fx.symlink(&format!("{d}/2-1:1.0/net/enx001122/device"), "../..");
        fx.symlink(&format!("{d}/2-1:1.0/driver"), "../../../../../../bus/usb/drivers/ax88179_178a");
        fx.symlink("sys/class/net/enx001122", "../../devices/pci0000:00/0000:00:14.0/usb2/2-1/2-1:1.0/net/enx001122");
        fx.symlink("sys/bus/usb/devices/2-1", "../../../devices/pci0000:00/0000:00:14.0/usb2/2-1");
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.parent.as_deref(), Some("usb:2-1"));
        assert_eq!(c.title, "AX88179 Gigabit Ethernet");
        assert_eq!(spec(c, "Bus").display(), "USB");
        assert_eq!(spec(c, "Bus address").display(), "2-1");
        assert_eq!(spec(c, "Vendor").display(), "ASIX Electronics Corp.");
    }

    #[test]
    fn cellular_by_name_and_unknown_type() {
        let fx = Fx::new(&[
            ("sys/class/net/wwan0/device/x", ""),
            ("sys/class/net/wwan0/type", "1\n"),
            ("sys/class/net/sit9/device/x", ""),
            ("sys/class/net/sit9/type", "776\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(spec(&v[1], "Type").display(), "Cellular");
        assert_eq!(spec(&v[0], "Type").display(), "Other");
        assert_eq!(v[0].parent, None);
    }

    #[test]
    fn empty_or_missing_tree() {
        assert!(collect_from(Fx::new(&[]).path()).is_empty());
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    #[test]
    fn non_utf8_mac_does_not_panic() {
        let fx = Fx::new(&[("sys/class/net/eth9/device/x", "")]);
        fx.write_bytes("sys/class/net/eth9/address", b"\xff\xfe\n");
        let c = &collect_from(fx.path())[0];
        assert!(spec(c, "MAC address").is_available());
    }
}
