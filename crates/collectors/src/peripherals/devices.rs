//! Bluetooth adapters, input devices, audio cards and cameras.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use super::ids::IdDb;
use super::{bus_parent, clean, link_name, opt_spec, pci_is_awake, pci_runtime_status, read_attr, read_hex, real_components, split_path};
use crate::util::{at, list_dir, text_spec};

fn proc_src(rel: &str) -> String {
    format!("procfs:/{}", rel.trim_start_matches('/'))
}

fn sys_src(rel: &str) -> String {
    crate::util::source(rel)
}

fn bus_label(parent: Option<&str>) -> Option<&'static str> {
    match parent {
        Some(p) if p.starts_with("usb:") => Some("USB"),
        Some(p) if p.starts_with("pci:") => Some("PCI"),
        _ => None,
    }
}

/// (vendor name from the ids databases, vendor string reported by the device) of a PCI/USB parent.
fn parent_vendor(root: &Path, parent: Option<&str>, usb: Option<&IdDb>, pci: Option<&IdDb>) -> (Option<String>, Option<String>) {
    match parent {
        Some(p) if p.starts_with("usb:") => {
            let base = format!("sys/bus/usb/devices/{}", &p[4..]);
            let v = read_hex(root, &format!("{base}/idVendor")).value().map(|v| *v as u16);
            (v.and_then(|v| usb?.vendor(v)).map(str::to_owned), read_attr(root, &format!("{base}/manufacturer")).value().cloned())
        }
        Some(p) if p.starts_with("pci:") => {
            let base = format!("sys/bus/pci/devices/{}", &p[4..]);
            let v = read_hex(root, &format!("{base}/vendor")).value().map(|v| *v as u16);
            (v.and_then(|v| pci?.vendor(v)).map(str::to_owned), None)
        }
        _ => (None, None),
    }
}

// ---------------------------------------------------------------------------------------------
// Bluetooth
// ---------------------------------------------------------------------------------------------

pub(super) fn bluetooth(root: &Path, usb: Option<&IdDb>, pci: Option<&IdDb>) -> Vec<Component> {
    list_dir(root, "sys/class/bluetooth")
        .into_iter()
        .filter(|n| n.starts_with("hci") && !n.contains(':'))
        .map(|n| {
            let base = format!("sys/class/bluetooth/{n}");
            let parent = bus_parent(&real_components(root, &base));
            let (vendor_db, vendor_str) = parent_vendor(root, parent.as_deref(), usb, pci);
            let vendor = vendor_db.or(vendor_str);
            let rf = list_dir(root, &base).into_iter().find(|e| e.starts_with("rfkill"));
            let radio = match &rf {
                Some(r) => {
                    let soft = read_attr(root, &format!("{base}/{r}/soft"));
                    let hard = read_attr(root, &format!("{base}/{r}/hard"));
                    match (soft.value().map(String::as_str), hard.value().map(String::as_str)) {
                        (_, Some("1")) => Spec::text("Radio", "Blocked by hardware switch", &sys_src(&format!("{base}/{r}/hard"))),
                        (Some("1"), _) => Spec::text("Radio", "Blocked by software", &sys_src(&format!("{base}/{r}/soft"))),
                        (Some("0"), Some("0")) => Spec::text("Radio", "On", &sys_src(&format!("{base}/{r}/soft"))),
                        _ => Spec::missing("Radio", &sys_src(&format!("{base}/{r}/soft"))),
                    }
                }
                None => Spec::missing("Radio", &sys_src(&format!("{base}/rfkill*/soft"))),
            };
            let conn = bus_label(parent.as_deref());
            let mut c = Component::new(&format!("bluetooth:{n}"), Category::Peripherals, &format!("Bluetooth adapter ({n})"));
            let sub = [conn.map(str::to_owned), vendor.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            if !sub.is_empty() {
                c = c.subtitle(&sub);
            }
            if let Some(p) = &parent {
                c = c.parent(p);
            }
            c.summary(vec![opt_spec("Vendor", vendor.clone(), &sys_src(&base)), radio.clone()])
                .group(SpecGroup::new("Identity", vec![
                    opt_spec("Vendor", vendor, &sys_src(&base)),
                    text_spec("Address", read_attr(root, &format!("{base}/address"))).identifier(),
                    Spec::missing("Bluetooth version", &sys_src(&format!("{base}/hci_version"))),
                ]))
                .group(SpecGroup::new("Connection", vec![
                    opt_spec("Connected via", conn.map(str::to_owned), &sys_src(&base)),
                    opt_spec("Parent device", parent.map(|p| p.split_once(':').map(|x| x.1.to_owned()).unwrap_or(p)), &sys_src(&base)),
                    radio.live(),
                ]))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Input devices
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
struct InputDev {
    bus: u32,
    vendor: u32,
    product: u32,
    version: u32,
    name: String,
    phys: String,
    sysfs: String,
    uniq: String,
    handlers: Vec<String>,
    ev: u64,
}

fn hex_kv(tok: &str, key: &str) -> Option<u32> {
    u32::from_str_radix(tok.strip_prefix(key)?.strip_prefix('=')?, 16).ok()
}

fn parse_input(bytes: &[u8]) -> Vec<InputDev> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut cur: Option<InputDev> = None;
    for line in text.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let Some(d) = cur.take() {
                out.push(d);
            }
            continue;
        }
        let Some((tag, rest)) = line.split_once(": ") else { continue };
        let d = cur.get_or_insert_with(InputDev::default);
        match tag {
            "I" => {
                for tok in rest.split_whitespace() {
                    if let Some(v) = hex_kv(tok, "Bus") {
                        d.bus = v;
                    } else if let Some(v) = hex_kv(tok, "Vendor") {
                        d.vendor = v;
                    } else if let Some(v) = hex_kv(tok, "Product") {
                        d.product = v;
                    } else if let Some(v) = hex_kv(tok, "Version") {
                        d.version = v;
                    }
                }
            }
            "N" => d.name = clean(rest.strip_prefix("Name=").unwrap_or(rest).trim().trim_matches('"')),
            "P" => d.phys = clean(rest.strip_prefix("Phys=").unwrap_or(rest)),
            "S" => d.sysfs = clean(rest.strip_prefix("Sysfs=").unwrap_or(rest)),
            "U" => d.uniq = clean(rest.strip_prefix("Uniq=").unwrap_or(rest)),
            "H" => d.handlers = rest.strip_prefix("Handlers=").unwrap_or(rest).split_whitespace().map(str::to_owned).collect(),
            "B" => {
                if let Some(v) = rest.strip_prefix("EV=") {
                    d.ev = u64::from_str_radix(v.trim(), 16).unwrap_or(0);
                }
            }
            _ => {}
        }
    }
    out
}

fn input_bus(bus: u32) -> String {
    match bus {
        0x00 => "Not specified".into(),
        0x01 => "PCI".into(),
        0x02 => "ISA".into(),
        0x03 => "USB".into(),
        0x05 => "Bluetooth".into(),
        0x06 => "Virtual".into(),
        0x11 => "i8042 (PS/2)".into(),
        0x18 => "I2C".into(),
        0x19 => "Platform".into(),
        0x1c => "SPI".into(),
        0x1f => "Intel ISH".into(),
        0x20 => "AMD SFH".into(),
        b => format!("Bus 0x{b:02x}"),
    }
}

fn input_kind(d: &InputDev) -> &'static str {
    let name = d.name.to_lowercase();
    let h = |x: &str| d.handlers.iter().any(|v| v == x || v.starts_with(x));
    if d.bus == 0x19 || ["power button", "sleep button", "lid switch", "video bus"].contains(&name.as_str()) {
        "Platform"
    } else if name.contains("hdmi") || name.contains("displayport") {
        "Audio output (HDMI/DP)"
    } else if name.contains("jack") || d.sysfs.contains("/sound/card") {
        "Audio jack"
    } else if name.contains("touchpad") {
        "Touchpad"
    } else if h("mouse") {
        "Pointing device"
    } else if h("js") {
        "Game controller"
    } else if h("kbd") && d.ev & (1 << 20) != 0 {
        "Keyboard"
    } else {
        "Input device"
    }
}

pub(super) fn input(root: &Path, usb: Option<&IdDb>) -> Vec<Component> {
    let rel = "proc/bus/input/devices";
    let Ok(bytes) = fs::read(at(root, rel)) else { return Vec::new() };
    let src = proc_src(rel);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (i, d) in parse_input(&bytes).into_iter().enumerate() {
        if d.name.is_empty() || d.bus == 0x06 || d.sysfs.contains("/devices/virtual/") {
            continue; // uinput, remappers and other software-made devices
        }
        let event = d.handlers.iter().find(|h| h.strip_prefix("event").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())));
        let mut id = format!("input:{}", event.cloned().unwrap_or_else(|| format!("device{i}")));
        if !seen.insert(id.clone()) {
            id = format!("{id}-{i}");
        }
        let kind = input_kind(&d);
        let parent = bus_parent(&split_path(&d.sysfs));
        let bus = input_bus(d.bus);
        let (vendor_name, product_name) = if d.bus == 0x03 {
            (usb.and_then(|u| u.vendor(d.vendor as u16)).map(str::to_owned), usb.and_then(|u| u.device(d.vendor as u16, d.product as u16)).map(str::to_owned))
        } else {
            (None, None)
        };
        let hex = |label: &str, v: u32| Spec::text(label, format!("0x{v:04x}"), &src);
        let mut c = Component::new(&id, Category::Peripherals, &d.name).subtitle(&if kind == bus { kind.to_owned() } else { format!("{kind} · {bus}") });
        if let Some(p) = &parent {
            c = c.parent(p);
        }
        out.push(
            c.summary(vec![Spec::text("Type", kind, &src), Spec::text("Connection", bus.clone(), &src)])
                .group(SpecGroup::new("Identity", vec![
                    Spec::text("Name", d.name.clone(), &src),
                    opt_spec("Vendor", vendor_name, &src),
                    opt_spec("Product", product_name, &src),
                    if d.uniq.is_empty() { Spec::missing("Unique id", &src) } else { Spec::text("Unique id", d.uniq.clone(), &src).identifier() },
                ]))
                .group(SpecGroup::new("Connection", vec![
                    Spec::text("Bus", bus, &src),
                    opt_spec("Physical path", if d.phys.is_empty() { None } else { Some(d.phys.clone()) }, &src),
                    opt_spec("Handlers", if d.handlers.is_empty() { None } else { Some(d.handlers.join(", ")) }, &src),
                ]))
                .group(SpecGroup::new("Raw identifiers", vec![
                    hex("Vendor ID", d.vendor),
                    hex("Product ID", d.product),
                    hex("Version", d.version),
                    Spec::text("Sysfs path", d.sysfs.clone(), &src),
                ]).technical()),
        );
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Audio cards
// ---------------------------------------------------------------------------------------------

struct Card {
    num: u32,
    id: String,
    kind: String,
    name: String,
}

fn parse_cards(text: &str) -> Vec<Card> {
    text.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let (num, rest) = t.split_once(' ')?;
            let num: u32 = num.parse().ok()?;
            let rest = rest.trim_start().strip_prefix('[')?;
            let (id, rest) = rest.split_once(']')?;
            let rest = rest.trim_start().strip_prefix(':')?.trim();
            let (kind, name) = rest.split_once(" - ").unwrap_or((rest, ""));
            Some(Card { num, id: clean(id), kind: clean(kind), name: clean(name) })
        })
        .collect()
}

/// Codec names ("Realtek ALC257") from `/proc/asound/cardN/codec#M`. Reading these makes the HDA
/// driver power the codec up, so callers only do it when the parent PCI function is already awake.
fn codec_names(root: &Path, num: u32) -> Vec<String> {
    let dir = format!("proc/asound/card{num}");
    list_dir(root, &dir)
        .into_iter()
        .filter(|n| n.starts_with("codec#"))
        .filter_map(|n| {
            let bytes = fs::read(at(root, &format!("{dir}/{n}"))).ok()?;
            let text = String::from_utf8_lossy(&bytes);
            let name = text.lines().find_map(|l| l.strip_prefix("Codec:")).map(clean)?;
            if name.is_empty() { None } else { Some(name) }
        })
        .collect()
}

pub(super) fn audio(root: &Path) -> Vec<Component> {
    let rel = "proc/asound/cards";
    let Ok(bytes) = fs::read(at(root, rel)) else { return Vec::new() };
    let src = proc_src(rel);
    parse_cards(&String::from_utf8_lossy(&bytes))
        .into_iter()
        .map(|card| {
            let n = card.num;
            let cbase = format!("sys/class/sound/card{n}");
            let parent = bus_parent(&real_components(root, &cbase));
            let driver = link_name(root, &format!("{cbase}/device/driver"));
            let title = if card.name.is_empty() { format!("Audio card {n}") } else { card.name.clone() };

            let codec_src = proc_src(&format!("proc/asound/card{n}/codec#*"));
            let codec: Field<Value> = match parent.as_deref() {
                Some(p) if p.starts_with("pci:") => {
                    let status = pci_runtime_status(root, &p[4..]);
                    if pci_is_awake(status.as_deref()) {
                        let names = codec_names(root, n);
                        if names.is_empty() { Field::not_reported(codec_src) } else { Field::available(Value::Text(names.join(", ")), codec_src) }
                    } else {
                        // Not read: the proc codec files would wake a runtime-suspended device.
                        Field::not_reported(codec_src)
                    }
                }
                Some(_) => Field::not_reported(codec_src),
                None => Field::not_reported(codec_src),
            };
            let conn = bus_label(parent.as_deref());
            let mut c = Component::new(&format!("audio:card{n}"), Category::Peripherals, &title);
            let sub = [Some("Audio".to_owned()), conn.map(str::to_owned), codec.value().and_then(|v| if let Value::Text(t) = v { Some(t.clone()) } else { None })].into_iter().flatten().collect::<Vec<_>>().join(" · ");
            c = c.subtitle(&sub);
            if let Some(p) = &parent {
                c = c.parent(p);
            }
            c.summary(vec![Spec::new("Codec", codec.clone()), opt_spec("Driver", driver.clone().or_else(|| Some(card.kind.clone()).filter(|k| !k.is_empty())), &src)])
                .group(SpecGroup::new("Identity", vec![
                    Spec::text("Card name", title, &src),
                    Spec::text("Short id", card.id.clone(), &src),
                    Spec::new("Codec", codec),
                ]))
                .group(SpecGroup::new("Connection", vec![
                    opt_spec("Connected via", conn.map(str::to_owned), &sys_src(&cbase)),
                    opt_spec("Parent device", parent.map(|p| p.split_once(':').map(|x| x.1.to_owned()).unwrap_or(p)), &sys_src(&cbase)),
                    opt_spec("Kernel driver", driver, &sys_src(&format!("{cbase}/device/driver"))),
                    Spec::text("ALSA driver type", card.kind, &src),
                    Spec::text("Card number", n.to_string(), &src),
                ]))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Cameras
// ---------------------------------------------------------------------------------------------

const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x1;
const V4L2_CAP_VIDEO_CAPTURE_MPLANE: u32 = 0x1000;
const V4L2_CAP_META_CAPTURE: u32 = 0x0080_0000;

pub(super) fn cameras(root: &Path) -> Vec<Component> {
    let mut seen_parents: HashSet<String> = HashSet::new();
    let mut nodes: Vec<(u32, String)> = list_dir(root, "sys/class/video4linux")
        .into_iter()
        .filter_map(|n| n.strip_prefix("video").and_then(|x| x.parse::<u32>().ok()).map(|i| (i, n.clone())))
        .collect();
    nodes.sort();
    let mut out = Vec::new();
    for (_, n) in nodes {
        let base = format!("sys/class/video4linux/{n}");
        // Metadata and secondary nodes: only the first node (index 0) of a function is a camera.
        if let Some(ix) = read_attr(root, &format!("{base}/index")).value()
            && ix != "0"
        {
            continue;
        }
        if let Some(caps) = read_hex(root, &format!("{base}/device_caps")).value()
            && (caps & (V4L2_CAP_VIDEO_CAPTURE | V4L2_CAP_VIDEO_CAPTURE_MPLANE) == 0 || caps & V4L2_CAP_META_CAPTURE != 0)
        {
            continue;
        }
        let comps = real_components(root, &base);
        let key = if comps.len() > 1 { comps[..comps.len() - 1].join("/") } else { n.clone() };
        if !seen_parents.insert(key) {
            continue;
        }
        let parent = bus_parent(&comps);
        let name = read_attr(root, &format!("{base}/name"));
        // V4L2 names read "<Product>: <Stream>" and the stream part is truncated by the kernel.
        let title = name.value().map(|n| n.split(": ").next().unwrap_or(n).to_owned()).unwrap_or_else(|| format!("Camera ({n})"));
        let driver = link_name(root, &format!("{base}/device/driver"));
        let conn = bus_label(parent.as_deref());
        let mut c = Component::new(&format!("camera:{n}"), Category::Peripherals, &title);
        let sub = [Some("Camera".to_owned()), conn.map(str::to_owned)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        c = c.subtitle(&sub);
        if let Some(p) = &parent {
            c = c.parent(p);
        }
        out.push(
            c.summary(vec![opt_spec("Connected via", conn.map(str::to_owned), &sys_src(&base)), opt_spec("Driver", driver.clone(), &sys_src(&format!("{base}/device/driver")))])
                .group(SpecGroup::new("Identity", vec![text_spec("Name", name)]))
                .group(SpecGroup::new("Connection", vec![
                    opt_spec("Parent device", parent.map(|p| p.split_once(':').map(|x| x.1.to_owned()).unwrap_or(p)), &sys_src(&base)),
                    opt_spec("Driver", driver, &sys_src(&format!("{base}/device/driver"))),
                    Spec::text("Video node", format!("/dev/{n}"), &sys_src(&base)),
                ])),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    const INPUT: &str = "I: Bus=0019 Vendor=0000 Product=0001 Version=0000\nN: Name=\"Power Button\"\nP: Phys=PNP0C0C/button/input0\nS: Sysfs=/devices/platform/PNP0C0C:00/input/input1\nU: Uniq=\nH: Handlers=kbd event1 \nB: PROP=0\nB: EV=3\n\nI: Bus=0011 Vendor=0001 Product=0001 Version=ab83\nN: Name=\"AT Translated Set 2 keyboard\"\nP: Phys=isa0060/serio0/input0\nS: Sysfs=/devices/platform/i8042/serio0/input/input3\nU: Uniq=\nH: Handlers=sysrq kbd leds event3 \nB: EV=120013\n\nI: Bus=0003 Vendor=046d Product=c52b Version=0111\nN: Name=\"Logitech USB Receiver\"\nP: Phys=usb-0000:00:14.0-2.3/input0\nS: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2.3/1-2.3:1.0/0003:046D:C52B.0003/input/input20\nU: Uniq=\nH: Handlers=mouse2 event5 \nB: EV=17\n\nI: Bus=0003 Vendor=1234 Product=0001 Version=0111\nN: Name=\"ydotoold virtual device\"\nP: Phys=\nS: Sysfs=/devices/virtual/input/input30\nU: Uniq=\nH: Handlers=event30 \nB: EV=7\n\nI: Bus=0001 Vendor=8086 Product=0001 Version=0000\nN: Name=\"HDA Intel PCH Headphone Mic\"\nP: Phys=ALSA\nS: Sysfs=/devices/pci0000:00/0000:00:1f.3/sound/card0/input10\nU: Uniq=\nH: Handlers=kbd event10 \nB: EV=21\n";

    #[test]
    fn input_devices_are_classified_and_virtual_ones_dropped() {
        let fx = Fx::new(&[("proc/bus/input/devices", INPUT), ("usr/share/hwdata/usb.ids", "046d  Logitech, Inc.\n\tc52b  Unifying Receiver\n")]);
        let v = input(fx.path(), IdDb::usb(fx.path()).as_ref());
        let ids: Vec<&str> = v.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["input:event1", "input:event3", "input:event5", "input:event10"]);
        assert_eq!(spec(&v[0], "Type").display(), "Platform");
        assert_eq!(spec(&v[1], "Type").display(), "Keyboard");
        assert_eq!(spec(&v[2], "Type").display(), "Pointing device");
        assert_eq!(spec(&v[2], "Vendor").display(), "Logitech, Inc.");
        assert_eq!(spec(&v[2], "Product").display(), "Unifying Receiver");
        assert_eq!(v[2].parent.as_deref(), Some("usb:1-2.3"));
        assert_eq!(v[3].parent.as_deref(), Some("pci:0000:00:1f.3"));
        assert_eq!(spec(&v[3], "Type").display(), "Audio jack");
        assert_eq!(v[0].parent, None);
    }

    #[test]
    fn input_garbage_and_missing() {
        let fx = Fx::new(&[]);
        assert!(input(fx.path(), None).is_empty());
        fx.write_bytes("proc/bus/input/devices", b"I: Bus=zz\nN: Name=\"Caf\xe9\"\nH: Handlers=\n\nnonsense\n\n");
        let v = input(fx.path(), None);
        assert_eq!(v.len(), 1);
        assert!(v[0].title.starts_with("Caf"));
        assert_eq!(spec(&v[0], "Handlers").display(), "Not reported");
    }

    fn sound_tree(status: &str) -> Fx {
        let fx = Fx::new(&[
            ("proc/asound/cards", " 0 [PCH            ]: HDA-Intel - HDA Intel PCH\n                      HDA Intel PCH at 0x6206130000 irq 158\n 2 [Gen            ]: USB-Audio - USB Audio Device\n                      Vendor USB Audio at usb-0000:00:14.0-3\n"),
            ("proc/asound/card0/codec#0", "Codec: Realtek ALC257\nAddress: 0\n"),
            ("sys/devices/pci0000:00/0000:00:1f.3/sound/card0/id", "PCH\n"),
            ("sys/devices/pci0000:00/0000:00:1f.3/driver_marker", ""),
            ("sys/devices/pci0000:00/0000:00:14.0/usb1/1-3/1-3:1.1/sound/card2/id", "Gen\n"),
            ("sys/bus/pci/devices/0000:00:1f.3/power/runtime_status", &format!("{status}\n")),
        ]);
        fx.symlink("sys/class/sound/card0", "../../devices/pci0000:00/0000:00:1f.3/sound/card0");
        fx.symlink("sys/class/sound/card2", "../../devices/pci0000:00/0000:00:14.0/usb1/1-3/1-3:1.1/sound/card2");
        fx.symlink("sys/devices/pci0000:00/0000:00:1f.3/driver", "../../../bus/pci/drivers/snd_hda_intel");
        fx.symlink("sys/devices/pci0000:00/0000:00:1f.3/sound/card0/device", "../..");
        fx
    }

    #[test]
    fn audio_cards_with_codec_when_awake() {
        let fx = sound_tree("active");
        let v = audio(fx.path());
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "audio:card0");
        assert_eq!(v[0].title, "HDA Intel PCH");
        assert_eq!(v[0].parent.as_deref(), Some("pci:0000:00:1f.3"));
        assert_eq!(spec(&v[0], "Codec").display(), "Realtek ALC257");
        assert_eq!(spec(&v[0], "Kernel driver").display(), "snd_hda_intel");
        assert_eq!(v[1].parent.as_deref(), Some("usb:1-3"));
        assert_eq!(spec(&v[1], "Codec").display(), "Not reported");
    }

    #[test]
    fn suspended_audio_function_is_not_read() {
        let fx = sound_tree("suspended");
        let v = audio(fx.path());
        assert!(matches!(spec(&v[0], "Codec").value.availability, Availability::NotReported));
        assert_eq!(v[0].title, "HDA Intel PCH");
    }

    #[test]
    fn audio_without_proc_is_empty() {
        assert!(audio(Fx::new(&[]).path()).is_empty());
        let fx = Fx::new(&[("proc/asound/cards", "--- no soundcards ---\n")]);
        assert!(audio(fx.path()).is_empty());
    }

    #[test]
    fn duplicate_camera_nodes_collapse_to_one() {
        let int = "sys/devices/pci0000:00/0000:00:14.0/usb1/1-11/1-11:1.0/video4linux";
        let fx = Fx::new(&[
            (&format!("{int}/video0/name"), "Integrated Camera: Integrated C\n"),
            (&format!("{int}/video0/index"), "0\n"),
            (&format!("{int}/video1/name"), "Integrated Camera: Integrated C\n"),
            (&format!("{int}/video1/index"), "1\n"),
            (&format!("{int}/video2/name"), "Integrated Camera: Integrated C\n"),
            (&format!("{int}/video2/index"), "0\n"),
            (&format!("{int}/video3/name"), "Meta\n"),
            (&format!("{int}/video3/index"), "0\n"),
            (&format!("{int}/video3/device_caps"), "0x84a00000\n"),
        ]);
        for i in 0..4 {
            fx.symlink(&format!("sys/class/video4linux/video{i}"), &format!("../../devices/pci0000:00/0000:00:14.0/usb1/1-11/1-11:1.0/video4linux/video{i}"));
        }
        fx.symlink("sys/devices/pci0000:00/0000:00:14.0/usb1/1-11/1-11:1.0/driver", "../../../../../../bus/usb/drivers/uvcvideo");
        fx.symlink(&format!("{int}/video0/device"), "../..");
        let v = cameras(fx.path());
        assert_eq!(v.len(), 1, "{v:#?}");
        assert_eq!(v[0].id, "camera:video0");
        assert_eq!(v[0].parent.as_deref(), Some("usb:1-11"));
        assert_eq!(spec(&v[0], "Driver").display(), "uvcvideo");
    }

    #[test]
    fn camera_without_index_or_name() {
        let fx = Fx::new(&[("sys/class/video4linux/video7/uevent", "")]);
        let v = cameras(fx.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].title, "Camera (video7)");
        assert!(cameras(Fx::new(&[]).path()).is_empty());
    }

    #[test]
    fn bluetooth_adapter_on_usb() {
        let dev = "sys/devices/pci0000:00/0000:00:14.0/usb1/1-14";
        let fx = Fx::new(&[
            (&format!("{dev}/idVendor"), "0489\n"),
            (&format!("{dev}/manufacturer"), "MediaTek Inc.\n"),
            (&format!("{dev}/1-14:1.0/bluetooth/hci0/rfkill4/soft"), "0\n"),
            (&format!("{dev}/1-14:1.0/bluetooth/hci0/rfkill4/hard"), "0\n"),
            (&format!("{dev}/1-14:1.0/bluetooth/hci0/hci0:512/x"), ""),
            ("usr/share/hwdata/usb.ids", "0489  Foxconn / Hon Hai\n"),
        ]);
        fx.symlink("sys/bus/usb/devices/1-14", "../../../devices/pci0000:00/0000:00:14.0/usb1/1-14");
        fx.symlink("sys/class/bluetooth/hci0", "../../devices/pci0000:00/0000:00:14.0/usb1/1-14/1-14:1.0/bluetooth/hci0");
        fx.symlink("sys/class/bluetooth/hci0:512", "../../devices/pci0000:00/0000:00:14.0/usb1/1-14/1-14:1.0/bluetooth/hci0/hci0:512");
        let usb = IdDb::usb(fx.path());
        let v = bluetooth(fx.path(), usb.as_ref(), None);
        assert_eq!(v.len(), 1, "connection handles like hci0:512 are not adapters");
        let c = &v[0];
        assert_eq!(c.id, "bluetooth:hci0");
        assert_eq!(c.parent.as_deref(), Some("usb:1-14"));
        assert_eq!(c.subtitle.as_deref(), Some("USB · Foxconn / Hon Hai"));
        assert_eq!(spec(c, "Radio").display(), "On");
        let addr = spec(c, "Address");
        assert!(addr.identifier);
        assert_eq!(addr.display(), "Not reported");
        assert_eq!(spec(c, "Bluetooth version").display(), "Not reported");
    }
}
