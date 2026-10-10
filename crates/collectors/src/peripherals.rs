//! Peripherals: PCI functions, USB devices, Bluetooth adapters, input devices, audio cards and
//! cameras, linked into a connection graph through `Component::parent`.
//!
//! Safety: this module never touches display-class PCI functions (the graphics collector owns them),
//! reads only config-cached attributes of other PCI functions, checks `power/runtime_status` before
//! anything that could reach the hardware, and never opens a device node.

use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component as PathPart, Path};

use facet_core::{Component, Field, Spec};

use crate::util::{at, source};

#[path = "peripherals/devices.rs"]
mod devices;
#[path = "peripherals/ids.rs"]
pub(crate) mod ids;
#[path = "peripherals/pci.rs"]
pub(crate) mod pci;
#[path = "peripherals/usb.rs"]
pub(crate) mod usb;

use ids::IdDb;

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let pci_db = IdDb::pci(root);
    let usb_db = IdDb::usb(root);
    let mut v = pci::collect(root, pci_db.as_ref());
    v.extend(usb::collect(root, usb_db.as_ref()));
    v.extend(devices::bluetooth(root, usb_db.as_ref(), pci_db.as_ref()));
    v.extend(devices::input(root, usb_db.as_ref()));
    v.extend(devices::audio(root));
    v.extend(devices::cameras(root));
    curate(&mut v);
    // A parent that is not itself a listed component (a bridge, a display function) is dropped.
    let ids: HashSet<String> = v.iter().map(|c| c.id.clone()).collect();
    for c in &mut v {
        if c.parent.as_ref().is_some_and(|p| !ids.contains(p) || *p == c.id) {
            c.parent = None;
        }
    }
    v
}

/// Drop input nodes that are noise to a person browsing hardware: platform buttons and lid
/// switches, ALSA jack-sense nodes (the audio card already represents them), the PC speaker, and
/// input nodes that merely duplicate the USB device they belong to.
fn curate(v: &mut Vec<Component>) {
    v.retain(|c| {
        if !c.id.starts_with("input:") {
            return true;
        }
        let kind = c.subtitle.as_deref().unwrap_or("");
        if matches!(kind, "Platform" | "Audio jack" | "Audio output (HDMI/DP)") || c.title == "PC Speaker" {
            return false;
        }
        !c.parent.as_deref().is_some_and(|p| p.starts_with("usb:"))
    });
}

// ---------------------------------------------------------------------------------------------
// Shared helpers (also used by network.rs)
// ---------------------------------------------------------------------------------------------

/// Strip control characters and surrounding whitespace; invalid UTF-8 was already replaced.
pub(crate) fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_owned()
}

/// Read a small sysfs attribute, tolerating non-UTF-8 bytes. Missing and EINVAL/ENODEV (attribute
/// exists but has no value right now, e.g. `speed` on a down link) mean "not reported".
pub(crate) fn read_attr(root: &Path, rel: &str) -> Field<String> {
    let src = source(rel);
    match fs::read(at(root, rel)) {
        Ok(b) => {
            let s = clean(&String::from_utf8_lossy(&b));
            if s.is_empty() { Field::not_reported(src) } else { Field::available(s, src) }
        }
        Err(e) => match e.kind() {
            ErrorKind::NotFound | ErrorKind::InvalidInput => Field::not_reported(src),
            ErrorKind::PermissionDenied => Field::permission_required(src),
            _ if matches!(e.raw_os_error(), Some(19) | Some(61)) => Field::not_reported(src),
            _ => Field::failed(e.to_string(), src),
        },
    }
}

pub(crate) fn parse_hex(s: &str) -> Result<u32, String> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    u32::from_str_radix(t, 16).map_err(|e| format!("invalid hex {s:?}: {e}"))
}

pub(crate) fn read_hex(root: &Path, rel: &str) -> Field<u32> {
    crate::util::parse_field(read_attr(root, rel), |s| parse_hex(s))
}

/// Last path component of a symlink's target (driver, subsystem, iommu_group).
pub(crate) fn link_name(root: &Path, rel: &str) -> Option<String> {
    let t = fs::read_link(at(root, rel)).ok()?;
    let n = t.file_name()?.to_string_lossy().into_owned();
    if n.is_empty() { None } else { Some(n) }
}

/// A text spec from an optional value; `None` is "not reported" with the given source.
pub(crate) fn opt_spec(label: &str, v: Option<String>, src: &str) -> Spec {
    match v {
        Some(t) => Spec::text(label, t, src),
        None => Spec::missing(label, src),
    }
}

pub(crate) fn is_pci_addr(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12
        && b[4] == b':'
        && b[7] == b':'
        && b[10] == b'.'
        && b.iter().enumerate().all(|(i, c)| matches!(i, 4 | 7 | 10) || c.is_ascii_hexdigit())
}

/// `usbN` root hubs and `<bus>-<port>(.<port>)*` devices; interfaces (`1-2:1.0`) are not devices.
pub(crate) fn is_usb_device_name(s: &str) -> bool {
    if let Some(n) = s.strip_prefix("usb") {
        return !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit());
    }
    let Some((bus, ports)) = s.split_once('-') else { return false };
    !bus.is_empty()
        && bus.bytes().all(|c| c.is_ascii_digit())
        && !ports.is_empty()
        && ports.split('.').all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
}

/// Components of the real (symlink-resolved) location of `rel`, relative to `root`. Falls back to the
/// textual target of the symlink when resolution fails.
pub(crate) fn real_components(root: &Path, rel: &str) -> Vec<String> {
    let p = at(root, rel);
    if let (Ok(c), Ok(r)) = (fs::canonicalize(&p), fs::canonicalize(root))
        && let Ok(stripped) = c.strip_prefix(&r)
    {
        return parts(stripped);
    }
    match fs::read_link(&p) {
        Ok(t) => parts(&t),
        Err(_) => Vec::new(),
    }
}

fn parts(p: &Path) -> Vec<String> {
    p.components()
        .filter_map(|c| match c {
            PathPart::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

pub(crate) fn split_path(p: &str) -> Vec<String> {
    p.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").map(str::to_owned).collect()
}

/// The deepest PCI function or USB device among `comps` as a component id (`pci:<addr>` or
/// `usb:<name>`). USB interface directories (`1-2:1.0`) never match.
pub(crate) fn bus_parent(comps: &[String]) -> Option<String> {
    comps.iter().rev().find_map(|c| {
        if is_pci_addr(c) {
            Some(format!("pci:{c}"))
        } else if is_usb_device_name(c) {
            Some(format!("usb:{c}"))
        } else {
            None
        }
    })
}

/// Runtime power state of a PCI function, or `None` if unreadable.
pub(crate) fn pci_runtime_status(root: &Path, addr: &str) -> Option<String> {
    read_attr(root, &format!("sys/bus/pci/devices/{addr}/power/runtime_status")).value().cloned()
}

/// True when it is safe to read config space of this PCI function (device is awake and stays so).
pub(crate) fn pci_is_awake(status: Option<&str>) -> bool {
    matches!(status, Some("active") | Some("unsupported"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;

    /// `cargo test -p facet-collectors live_dump -- --ignored --nocapture` prints this machine's view.
    #[test]
    #[ignore]
    fn live_dump() {
        for c in crate::network::collect().into_iter().chain(collect()) {
            println!("{} | {} | {} | parent={:?}", c.id, c.title, c.subtitle.clone().unwrap_or_default(), c.parent);
            for s in &c.summary {
                println!("      {}: {}", s.label, if s.identifier { "<id>".into() } else { s.display() });
            }
        }
    }

    /// A reduced copy of this machine's real tree (Lenovo Legion 5 15IRX10, 2026-10-09): hybrid
    /// graphics with a runtime-suspended NVIDIA GPU, xHCI with a camera, a Bluetooth radio and an
    /// ITE keyboard, HDA audio, Realtek Ethernet and a MediaTek Wi-Fi card.
    #[test]
    fn real_machine_fixture_end_to_end() {
        let fx = Fx::new(&[
            ("usr/share/hwdata/pci.ids", "10de  NVIDIA Corporation\n\t2d58  GB206M [GeForce RTX 5070 Max-Q]\n\t22eb  GB206 High Definition Audio Controller\n8086  Intel Corporation\n\t7ae0  700 Series Chipset Family USB 3.2 xHCI Host Controller\n10ec  Realtek Semiconductor Co., Ltd.\n\t8168  RTL8111/8168 Gigabit Ethernet\n14c3  MEDIATEK Corp.\n\t7925  MT7925 802.11be\nC 02  Network controller\n\t00  Ethernet controller\n\t80  Network controller\nC 03  Display controller\nC 04  Multimedia controller\n\t03  Audio device\nC 0c  Serial bus controller\n\t03  USB controller\n"),
            ("usr/share/hwdata/usb.ids", "0489  Foxconn / Hon Hai\n1d6b  Linux Foundation\n\t0002  2.0 root hub\n30c9  Luxvisions Innotech Limited\nC 09  Hub\nC 0e  Video\nC e0  Wireless\nC ef  Miscellaneous Device\n"),
            ("proc/asound/cards", " 0 [PCH            ]: HDA-Intel - HDA Intel PCH\n                      HDA Intel PCH at 0x6206130000 irq 158\n 1 [NVidia         ]: HDA-Intel - HDA NVidia\n                      HDA NVidia at 0x88080000 irq 17\n"),
            ("proc/asound/card0/codec#0", "Codec: Realtek ALC257\n"),
            ("proc/asound/card1/codec#2", "Codec: Nvidia GPU 99 HDMI/DP\n"),
        ]);
        let pci = |a: &str| format!("sys/devices/pci0000:00/{a}");
        for (path, addr, class, ven, dev, st) in [
            ("0000:00:01.0/0000:01:00.0", "0000:01:00.0", "0x030000", "0x10de", "0x2d58", "suspended"),
            ("0000:00:01.0/0000:01:00.1", "0000:01:00.1", "0x040300", "0x10de", "0x22eb", "suspended"),
            ("0000:00:14.0", "0000:00:14.0", "0x0c0330", "0x8086", "0x7ae0", "active"),
            ("0000:00:1b.5/0000:0c:00.0", "0000:0c:00.0", "0x020000", "0x10ec", "0x8168", "active"),
            ("0000:00:1c.4/0000:12:00.0", "0000:12:00.0", "0x028000", "0x14c3", "0x7925", "active"),
            ("0000:00:1f.3", "0000:00:1f.3", "0x040300", "0x8086", "0x51ca", "suspended"),
        ] {
            fx.write(&format!("{}/class", pci(path)), &format!("{class}\n"));
            fx.write(&format!("{}/vendor", pci(path)), &format!("{ven}\n"));
            fx.write(&format!("{}/device", pci(path)), &format!("{dev}\n"));
            fx.write(&format!("{}/power/runtime_status", pci(path)), &format!("{st}\n"));
            fx.write(&format!("{}/current_link_speed", pci(path)), "16.0 GT/s PCIe\n");
            fx.symlink(&format!("sys/bus/pci/devices/{addr}"), &format!("../../../devices/pci0000:00/{path}"));
        }
        // Integrated function without a PCIe link reports "Unknown speed".
        fx.write(&format!("{}/current_link_speed", pci("0000:00:14.0")), "Unknown\n");
        // USB
        let xhci = pci("0000:00:14.0");
        for (n, vid, pidv, class, spd, ver, prod) in [
            ("usb1", "1d6b", "0002", "09", "480", " 2.00", "xHCI Host Controller"),
            ("usb1/1-11", "30c9", "00f8", "ef", "480", " 2.01", "Integrated Camera"),
            ("usb1/1-14", "0489", "e111", "ef", "480", " 2.10", "Wireless_Device"),
        ] {
            let d = format!("{xhci}/{n}");
            fx.write(&format!("{d}/idVendor"), &format!("{vid}\n"));
            fx.write(&format!("{d}/idProduct"), &format!("{pidv}\n"));
            fx.write(&format!("{d}/bDeviceClass"), &format!("{class}\n"));
            fx.write(&format!("{d}/speed"), &format!("{spd}\n"));
            fx.write(&format!("{d}/version"), &format!("{ver}\n"));
            fx.write(&format!("{d}/product"), &format!("{prod}\n"));
            fx.symlink(&format!("sys/bus/usb/devices/{}", n.rsplit('/').next().unwrap()), &format!("../../../devices/pci0000:00/0000:00:14.0/{n}"));
        }
        fx.write(&format!("{xhci}/usb1/1-11/1-11:1.0/bInterfaceClass"), "0e\n");
        fx.symlink("sys/bus/usb/devices/1-11:1.0", "../../../devices/pci0000:00/0000:00:14.0/usb1/1-11/1-11:1.0");
        // Camera: two nodes on one interface
        for (i, idx) in [(0, "0"), (1, "1")] {
            fx.write(&format!("{xhci}/usb1/1-11/1-11:1.0/video4linux/video{i}/name"), "Integrated Camera: Integrated C\n");
            fx.write(&format!("{xhci}/usb1/1-11/1-11:1.0/video4linux/video{i}/index"), &format!("{idx}\n"));
            fx.symlink(&format!("sys/class/video4linux/video{i}"), &format!("../../devices/pci0000:00/0000:00:14.0/usb1/1-11/1-11:1.0/video4linux/video{i}"));
        }
        // Bluetooth, sound, net
        fx.write(&format!("{xhci}/usb1/1-14/1-14:1.0/bluetooth/hci0/rfkill4/soft"), "0\n");
        fx.symlink("sys/class/bluetooth/hci0", "../../devices/pci0000:00/0000:00:14.0/usb1/1-14/1-14:1.0/bluetooth/hci0");
        fx.write(&format!("{}/sound/card0/id", pci("0000:00:1f.3")), "PCH\n");
        fx.write(&format!("{}/sound/card1/id", pci("0000:00:01.0/0000:01:00.1")), "NVidia\n");
        fx.symlink("sys/class/sound/card0", "../../devices/pci0000:00/0000:00:1f.3/sound/card0");
        fx.symlink("sys/class/sound/card1", "../../devices/pci0000:00/0000:00:01.0/0000:01:00.1/sound/card1");
        for (p, n, extra) in [("0000:00:1b.5/0000:0c:00.0", "enp12s0", "down"), ("0000:00:1c.4/0000:12:00.0", "wlp18s0", "up")] {
            fx.write(&format!("{}/net/{n}/operstate", pci(p)), &format!("{extra}\n"));
            fx.write(&format!("{}/net/{n}/type", pci(p)), "1\n");
            fx.symlink(&format!("{}/net/{n}/device", pci(p)), "../..");
            fx.symlink(&format!("sys/class/net/{n}"), &format!("../../devices/pci0000:00/{p}/net/{n}"));
        }
        fx.write("sys/devices/virtual/net/tailscale0/operstate", "unknown\n");
        fx.symlink("sys/class/net/tailscale0", "../../devices/virtual/net/tailscale0");
        fx.write("proc/bus/input/devices", "I: Bus=0003 Vendor=048d Product=c195 Version=0100\nN: Name=\"ITE Device Keyboard\"\nP: Phys=usb-0000:00:14.0-6/input0\nS: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-6/1-6:1.0/0003:048D:C195.0001/input/input5\nU: Uniq=\nH: Handlers=sysrq kbd event5 leds \nB: EV=120013\n\n");

        let per = collect_from(fx.path());
        let ids: Vec<&str> = per.iter().map(|c| c.id.as_str()).collect();
        // Display function and bridges never appear; the dGPU's audio function does (non-display) but unread.
        assert!(!ids.contains(&"pci:0000:01:00.0"), "{ids:?}");
        for id in ["pci:0000:01:00.1", "pci:0000:00:14.0", "usb:usb1", "usb:1-11", "usb:1-14", "bluetooth:hci0", "camera:video0", "audio:card0", "audio:card1"] {
            assert!(ids.contains(&id), "missing {id} in {ids:?}");
        }
        assert!(!ids.contains(&"camera:video1"));
        assert!(!ids.contains(&"input:event5"), "input nodes under a USB device are folded into it: {ids:?}");
        let get = |id: &str| per.iter().find(|c| c.id == id).unwrap();
        assert_eq!(get("usb:usb1").parent.as_deref(), Some("pci:0000:00:14.0"));
        assert_eq!(get("usb:1-14").parent.as_deref(), Some("usb:usb1"));
        assert_eq!(get("bluetooth:hci0").parent.as_deref(), Some("usb:1-14"));
        assert_eq!(get("camera:video0").parent.as_deref(), Some("usb:1-11"));
        assert_eq!(get("audio:card1").parent.as_deref(), Some("pci:0000:01:00.1"));
        let spec = |c: &Component, l: &str| c.all_specs().find(|s| s.label == l).unwrap().display();
        // Safety: a suspended audio function is neither link-probed nor codec-read.
        assert_eq!(spec(get("audio:card1"), "Codec"), "Not reported");
        assert_eq!(spec(get("audio:card0"), "Codec"), "Not reported");
        assert_eq!(spec(get("pci:0000:01:00.1"), "Current link speed"), "Not reported");
        assert_eq!(spec(get("pci:0000:01:00.1"), "Runtime power state"), "suspended");
        assert_eq!(spec(get("pci:0000:00:14.0"), "Current link speed"), "Not reported", "'Unknown' is no link");
        // Wake the sound card (simulating runtime_status=active) and the codec becomes readable.
        fx.write(&format!("{}/power/runtime_status", pci("0000:00:1f.3")), "active\n");
        let per2 = collect_from(fx.path());
        assert_eq!(spec(per2.iter().find(|c| c.id == "audio:card0").unwrap(), "Codec"), "Realtek ALC257");

        let net = crate::network::collect_from(fx.path());
        assert_eq!(net.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["network:enp12s0", "network:wlp18s0"]);
        for n in &net {
            let p = n.parent.as_ref().unwrap();
            assert!(ids.contains(&p.as_str()), "network parent {p} must be a peripherals component");
        }
    }

    #[test]
    fn name_patterns() {
        assert!(is_pci_addr("0000:0c:00.0"));
        assert!(!is_pci_addr("pci0000:00"));
        assert!(!is_pci_addr("0000:0c:00.0x"));
        assert!(is_usb_device_name("usb1"));
        assert!(is_usb_device_name("1-2.3.4"));
        assert!(!is_usb_device_name("1-2:1.0"));
        assert!(!is_usb_device_name("usb"));
        assert!(!is_usb_device_name("1-"));
    }

    #[test]
    fn bus_parent_picks_deepest() {
        let c = split_path("/devices/pci0000:00/0000:00:14.0/usb1/1-6/1-6:1.0/0003:048D:C195.0001/input/input5");
        assert_eq!(bus_parent(&c).as_deref(), Some("usb:1-6"));
        let c = split_path("/devices/pci0000:00/0000:00:1f.3/sound/card0/input10");
        assert_eq!(bus_parent(&c).as_deref(), Some("pci:0000:00:1f.3"));
        assert_eq!(bus_parent(&split_path("/devices/platform/PNP0C0C:00/input/input1")), None);
    }

    #[test]
    fn read_attr_handles_non_utf8_and_missing() {
        let fx = Fx::new(&[]);
        fx.write_bytes("sys/a", b"Caf\xe9 \x01Cam\n");
        let f = read_attr(fx.path(), "sys/a");
        assert!(f.value().unwrap().starts_with("Caf"));
        assert!(!f.value().unwrap().contains('\u{1}'));
        assert_eq!(read_attr(fx.path(), "sys/none").value(), None);
    }

    #[test]
    fn empty_and_unreadable_roots_yield_nothing() {
        let fx = Fx::new(&[]);
        assert!(collect_from(fx.path()).is_empty());
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    #[test]
    fn dangling_parent_is_dropped() {
        // An input device whose sysfs parent is a bridge/display function must not point at it.
        let fx = Fx::new(&[(
            "proc/bus/input/devices",
            "I: Bus=0001 Vendor=0000 Product=0000 Version=0000\nN: Name=\"Odd thing\"\nP: Phys=\nS: Sysfs=/devices/pci0000:00/0000:00:01.0/input/input9\nU: Uniq=\nH: Handlers=event9 \nB: EV=0\n\n",
        )]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].parent, None);
    }

    #[test]
    fn curate_drops_noise_but_keeps_real_inputs() {
        let mk = |id: &str, title: &str, kind: &str, parent: Option<&str>| {
            let mut c = Component::new(id, facet_core::Category::Peripherals, title).subtitle(kind);
            c.parent = parent.map(str::to_owned);
            c
        };
        let mut v = vec![
            mk("input:event0", "Power Button", "Platform", None),
            mk("input:event1", "HDA Intel PCH Mic", "Audio jack", None),
            mk("input:event2", "PC Speaker", "Input device", None),
            mk("input:event3", "ITE Keyboard", "Keyboard", Some("usb:1-6")),
            mk("input:event4", "AT keyboard", "Keyboard", None),
            mk("usb:1-6", "ITE Device", "USB", None),
        ];
        curate(&mut v);
        let ids: Vec<&str> = v.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["input:event4", "usb:1-6"]);
    }
}
