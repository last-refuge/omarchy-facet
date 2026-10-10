//! Connected displays from DRM connectors in sysfs plus the binary EDID each exposes.
//!
//! One component per CONNECTED connector (`/sys/class/drm/cardN-<name>/status == connected`). Reading
//! `status` or `edid` makes the kernel probe the connector, which can resume a runtime-suspended GPU, so
//! connectors of any GPU that is not `active` are skipped without being opened. No ioctls, no /dev/dri.

#[path = "edid.rs"]
mod edid;

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::graphics::{Power, card_pci_addr, power_of};
use crate::util::{at, list_dir, read_field, source, text_spec};
use edid::Edid;

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let mut out = Vec::new();
    for name in list_dir(root, "sys/class/drm") {
        let Some((card, _)) = name.split_once('-') else { continue };
        if !is_card(card) {
            continue;
        }
        let addr = card_pci_addr(root, card);
        if let Some(a) = &addr
            && power_of(root, a) != Power::Awake
        {
            continue; // never probe a connector on a GPU that is not active
        }
        if read_field(root, &format!("sys/class/drm/{name}/status")).value().map(String::as_str) != Some("connected") {
            continue;
        }
        out.push(read_connector(root, &name, addr.as_deref()));
    }
    out
}

fn is_card(s: &str) -> bool {
    s.strip_prefix("card").is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
}

/// Human description of a connector name such as `eDP-1`, `DP-3`, `HDMI-A-2`.
fn connector_kind(port: &str) -> &'static str {
    let base = port.rsplit_once('-').map_or(port, |(b, _)| b);
    match base {
        "eDP" => "Internal panel (eDP)",
        "LVDS" => "Internal panel (LVDS)",
        "DP" => "DisplayPort",
        "HDMI-A" | "HDMI-B" => "HDMI",
        "DVI-D" | "DVI-I" | "DVI-A" => "DVI",
        "VGA" => "VGA",
        "Virtual" => "Virtual output",
        "Unknown" => "Unknown",
        _ => "Other connector",
    }
}

/// EDID file contents, or the honest reason there are none.
fn read_edid(root: &Path, rel: &str) -> Result<Edid, Field<String>> {
    let src = source(rel);
    match fs::read(at(root, rel)) {
        Ok(b) if b.is_empty() => Err(Field::not_reported(format!("{src} (empty: the panel or cable returned no EDID)"))),
        Ok(b) => edid::parse(&b).map_err(|e| Field::failed(e.to_string(), src)),
        Err(e) => Err(match e.kind() {
            ErrorKind::NotFound => Field::not_reported(src),
            ErrorKind::PermissionDenied => Field::permission_required(src),
            _ => Field::failed(e.to_string(), src),
        }),
    }
}

struct Ed {
    src: String,
    parsed: Result<Edid, Field<String>>,
}

impl Ed {
    fn spec(&self, label: &str, f: impl FnOnce(&Edid) -> Option<String>) -> Spec {
        match &self.parsed {
            Ok(e) => match f(e) {
                Some(t) => Spec::text(label, t, &self.src),
                None => Spec::missing(label, &self.src),
            },
            Err(fld) => text_spec(label, fld.clone()),
        }
    }
}

fn mode_text(t: &edid::Timing) -> String {
    format!("{} × {}{}", t.h_active, t.v_active, if t.interlaced { "i" } else { "" })
}

fn refresh_text(t: &edid::Timing) -> Option<String> {
    t.refresh_hz().map(|hz| format!("{hz:.2} Hz").replace(".00 Hz", " Hz"))
}

fn read_connector(root: &Path, conn: &str, addr: Option<&str>) -> Component {
    let port = conn.split_once('-').map_or(conn, |(_, p)| p);
    let dir = format!("sys/class/drm/{conn}");
    let ed = Ed { src: source(&format!("{dir}/edid")), parsed: read_edid(root, &format!("{dir}/edid")) };
    let modes_f = read_field(root, &format!("{dir}/modes"));
    let enabled = read_field(root, &format!("{dir}/enabled"));
    let mode_list: Vec<String> = modes_f.value().map(|m| m.lines().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty()).collect()).unwrap_or_default();

    let e = ed.parsed.as_ref().ok();
    let vendor = e.map(|e| match edid::vendor_name(&e.pnp) {
        Some(n) => format!("{n} ({})", e.pnp),
        None => e.pnp.clone(),
    });
    let title = match e {
        Some(e) => match (&e.model_name, edid::vendor_name(&e.pnp)) {
            (Some(m), Some(v)) if !m.to_lowercase().starts_with(&v.to_lowercase()) => format!("{v} {m}"),
            (Some(m), _) => m.clone(),
            (None, Some(v)) => format!("{v} display"),
            (None, None) => format!("{} display", e.pnp),
        },
        None => format!("Display on {port}"),
    };
    let size = e.and_then(|e| e.width_cm.zip(e.height_cm)).map(|(w, h)| format!("{:.1} inches ({w} × {h} cm)", edid::diagonal_inches(w, h)));
    let pref = e.and_then(|e| e.preferred.as_ref());
    let mut sub: Vec<String> = Vec::new();
    if let Some((w, h)) = e.and_then(|e| e.width_cm.zip(e.height_cm)) {
        sub.push(format!("{:.1}\"", edid::diagonal_inches(w, h)));
    }
    if let Some(t) = pref {
        sub.push(match refresh_text(t) {
            Some(r) => format!("{} @ {r}", mode_text(t)),
            None => mode_text(t),
        });
    }
    sub.push(port.to_owned());

    let card = conn.split_once('-').map_or("", |(c, _)| c);
    let mut c = Component::new(&format!("display:{conn}"), Category::Displays, &title).subtitle(&sub.join(" · "));
    if let Some(a) = addr {
        c = c.parent(&format!("graphics:{a}"));
    }

    let active = match (enabled.value().map(String::as_str), mode_list.first()) {
        (Some("enabled"), Some(m)) => Spec::text("Active mode", m, &format!("{} (first listed mode; the kernel lists the preferred one first, the compositor may use another)", source(&format!("{dir}/modes")))),
        (Some("enabled"), None) => Spec::missing("Active mode", &source(&format!("{dir}/modes"))),
        (Some(_), _) => Spec::new("Active mode", Field::not_reported(format!("{} (connector is not enabled)", source(&format!("{dir}/enabled"))))),
        (None, _) => Spec::new("Active mode", Field::not_reported(source(&format!("{dir}/enabled")))),
    };

    c = c.summary(vec![
        ed.spec("Screen size", |_| size.clone()),
        ed.spec("Native resolution", |_| pref.map(mode_text)),
        ed.spec("Refresh rate", |_| pref.and_then(refresh_text)),
        Spec::text("Connection", connector_kind(port), &source(&format!("{dir}/status"))),
    ]);
    c = c.group(SpecGroup::new("Identity", vec![
        ed.spec("Manufacturer", |_| vendor.clone()),
        ed.spec("Model", |e| e.model_name.clone()),
        ed.spec("Product code", |e| Some(format!("0x{:04X}", e.product_code))),
        ed.spec("Serial text", |e| e.serial_text.clone()).identifier(),
        ed.spec("Serial number", |e| e.serial_number.map(|n| n.to_string())).identifier(),
        ed.spec("Manufactured", |e| {
            let y = e.year?;
            Some(match (e.model_year, e.week) {
                (true, _) => format!("Model year {y}"),
                (false, Some(w)) => format!("Week {w} of {y}"),
                (false, None) => y.to_string(),
            })
        }),
    ]));
    c = c.group(SpecGroup::new("Panel", vec![
        ed.spec("Physical size", |_| size.clone()),
        ed.spec("Preferred resolution", |_| pref.map(mode_text)),
        ed.spec("Preferred refresh rate", |_| pref.and_then(refresh_text)),
        ed.spec("Input type", |e| {
            let kind = if e.digital { "Digital" } else { "Analog" };
            Some(match e.interface {
                Some(i) => format!("{kind} ({i})"),
                None => kind.to_owned(),
            })
        }),
        ed.spec("Colour depth", |e| e.bit_depth.map(|b| format!("{b} bits per colour"))),
        ed.spec("Colour formats", |e| if e.color_formats.is_empty() { None } else { Some(e.color_formats.join(", ")) }),
        ed.spec("HDR", |e| {
            if e.hdr_eotfs.is_empty() {
                return Some("Not declared in EDID".to_owned());
            }
            let mut t = e.hdr_eotfs.join(", ");
            if let Some(n) = e.hdr_max_luminance_nits {
                t.push_str(&format!("; peak luminance about {n:.0} cd/m²"));
            }
            Some(t)
        }),
        ed.spec("Colorimetry", |e| if e.colorimetry.is_empty() { None } else { Some(e.colorimetry.join(", ")) }),
        Spec::new("Enabled", match enabled.value().map(String::as_str) {
            Some("enabled") => Field::available(Value::Bool(true), enabled.source.clone()),
            Some("disabled") => Field::available(Value::Bool(false), enabled.source.clone()),
            Some(o) => Field::failed(format!("unexpected value {o:?}"), enabled.source.clone()),
            None => crate::util::parse_field(enabled.clone(), |_| Ok(Value::Bool(false))),
        }),
        active,
    ]));
    let modes_spec = match modes_f.value() {
        Some(_) => Spec::text("Supported modes", mode_list.join(", "), &source(&format!("{dir}/modes"))),
        None => text_spec("Supported modes", modes_f.clone()),
    };
    c.group(SpecGroup::new("Technical", vec![
        modes_spec,
        Spec::val("Mode count", Value::Count(mode_list.len() as u64), &source(&format!("{dir}/modes"))),
        ed.spec("EDID version", |e| Some(format!("{}.{}", e.version, e.revision))),
        ed.spec("EDID integrity", |e| {
            Some(if e.warnings.is_empty() {
                format!("Valid ({} extension block(s))", e.extension_blocks)
            } else {
                format!("Warnings: {}", e.warnings.join("; "))
            })
        }),
        ed.spec("Timing source", |e| e.preferred.as_ref().map(|t| t.origin.to_owned())),
        Spec::text("Connector", conn, &source(&dir)),
        Spec::text("Graphics device", addr.unwrap_or("not a PCI device"), &source(&format!("sys/class/drm/{card}/device"))),
    ]).technical())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    /// Verbatim `/sys/class/drm/card1-eDP-1/edid` from the reference machine (EDO panel, base block,
    /// CTA-861 extension with HDR metadata, DisplayID 2.0 extension carrying the 2560x1600@165 timing).
    const REAL_EDID: [u8; 384] = [
    0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x14, 0x8f, 0x45, 0x42, 0x00, 0x00, 0x00, 0x00,
    0x19, 0x22, 0x01, 0x04, 0xb5, 0x20, 0x14, 0x78, 0x02, 0xd0, 0x45, 0xae, 0x51, 0x3e, 0xba, 0x23,
    0x0b, 0x50, 0x54, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
    0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xfe, 0x00, 0x45,
    0x44, 0x4f, 0x20, 0x53, 0x48, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfc,
    0x00, 0x45, 0x46, 0x31, 0x30, 0x51, 0x42, 0x43, 0x36, 0x34, 0x2e, 0x43, 0x0a, 0x20, 0x02, 0x0b,
    0x02, 0x03, 0x1d, 0x00, 0xe3, 0x05, 0x80, 0x00, 0xe6, 0x06, 0x05, 0x01, 0x8b, 0x6b, 0x02, 0x6d,
    0x1a, 0x00, 0x00, 0x03, 0x02, 0x3c, 0xa5, 0x00, 0xa0, 0x8b, 0x02, 0x6b, 0x02, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x85,
    0x70, 0x20, 0x79, 0x02, 0x00, 0x20, 0x00, 0x0c, 0x38, 0xec, 0x11, 0x45, 0x42, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x18, 0x00, 0x21, 0x00, 0x1d, 0x80, 0x0c, 0xd0, 0x07, 0x00, 0x0a, 0x40, 0x06, 0x00,
    0xee, 0x2a, 0x51, 0xdf, 0x23, 0xba, 0x35, 0x02, 0x0b, 0x02, 0x45, 0x54, 0xee, 0x5f, 0xee, 0x63,
    0xea, 0x10, 0x23, 0x78, 0x26, 0x00, 0x09, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00,
    0x22, 0x00, 0x28, 0x73, 0xf1, 0x0b, 0x05, 0xff, 0x09, 0x9f, 0x00, 0x4f, 0x00, 0x1f, 0x00, 0x3f,
    0x06, 0x7b, 0x0c, 0x31, 0x8c, 0x07, 0x00, 0x73, 0xf1, 0x0b, 0x85, 0xff, 0x09, 0x9f, 0x00, 0x4f,
    0x00, 0x1f, 0x00, 0x3f, 0x06, 0x8f, 0x00, 0x45, 0x80, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x90,
    ];

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    fn put_connector(fx: &Fx, conn: &str, status: &str, enabled: &str, modes: &str, edid: Option<&[u8]>) {
        let d = format!("sys/class/drm/{conn}");
        fx.write(&format!("{d}/status"), &format!("{status}\n"));
        fx.write(&format!("{d}/enabled"), &format!("{enabled}\n"));
        fx.write(&format!("{d}/modes"), modes);
        if let Some(b) = edid {
            fx.write_bytes(&format!("{d}/edid"), b);
        }
    }

    fn card(fx: &Fx, card: &str, addr: &str, status: &str) {
        fx.symlink(&format!("sys/class/drm/{card}/device"), &format!("../../../{addr}"));
        fx.write(&format!("sys/bus/pci/devices/{addr}/power/runtime_status"), &format!("{status}\n"));
    }

    /// Build a 128-byte EDID 1.4 base block with a valid checksum.
    fn make_edid(pnp: &str, product: u16, serial: u32, name: Option<&str>, serial_text: Option<&str>, cm: (u8, u8)) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        let l: Vec<u16> = pnp.bytes().map(|c| u16::from(c - b'A' + 1)).collect();
        let m = (l[0] << 10) | (l[1] << 5) | l[2];
        b[8..10].copy_from_slice(&m.to_be_bytes());
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&serial.to_le_bytes());
        b[16] = 12;
        b[17] = 30; // 2020
        b[18] = 1;
        b[19] = 4;
        b[20] = 0xa5; // digital, 8 bpc, DisplayPort
        b[21] = cm.0;
        b[22] = cm.1;
        b[24] = 0x08; // RGB + YCbCr 4:4:4
        b[54..72].copy_from_slice(&[0x02, 0x3a, 0x80, 0x18, 0x71, 0x38, 0x2d, 0x40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1e]);
        let mut desc = |i: usize, tag: u8, text: &str| {
            let o = 72 + 18 * i;
            b[o + 3] = tag;
            let mut t = text.as_bytes().to_vec();
            t.push(0x0a);
            t.resize(13, b' ');
            b[o + 5..o + 18].copy_from_slice(&t);
        };
        if let Some(n) = name {
            desc(0, 0xfc, n);
        }
        if let Some(s) = serial_text {
            desc(1, 0xff, s);
        }
        let sum = b[..127].iter().fold(0u8, |a, x| a.wrapping_add(*x));
        b[127] = 0u8.wrapping_sub(sum);
        b
    }

    #[test]
    fn real_machine_edid_parses_like_edid_decode() {
        let e = edid::parse(&REAL_EDID).unwrap();
        assert_eq!((e.version, e.revision), (1, 4));
        assert_eq!(e.pnp, "EDO");
        assert_eq!(e.product_code, 16965);
        assert_eq!(e.model_name.as_deref(), Some("EF10QBC64.C"));
        assert_eq!((e.week, e.year), (Some(25), Some(2024)));
        assert!(e.digital);
        assert_eq!(e.bit_depth, Some(10));
        assert_eq!(e.interface, Some("DisplayPort"));
        assert_eq!((e.width_cm, e.height_cm), (Some(32), Some(20)));
        assert_eq!(e.extension_blocks, 2);
        assert!(e.base_checksum_ok && e.extension_checksums_ok, "{:?}", e.warnings);
        assert!(e.warnings.is_empty());
        assert_eq!(e.hdr_eotfs, vec!["SDR gamma", "SMPTE ST 2084 (PQ)"]);
        assert_eq!(e.hdr_max_luminance_nits.map(|n| n.round() as u32), Some(1015));
        assert_eq!(e.colorimetry, vec!["BT.2020 RGB"]);
        let t = e.preferred.unwrap();
        assert_eq!((t.h_active, t.v_active, t.origin), (2560, 1600, "DisplayID extension"));
        assert_eq!(t.refresh_hz().unwrap().round() as u32, 165);
        assert_eq!(e.serial_number, None);
    }

    #[test]
    fn real_edid_component_for_the_internal_panel() {
        let fx = Fx::new(&[]);
        card(&fx, "card1", "0000:00:02.0", "active");
        put_connector(&fx, "card1-eDP-1", "connected", "enabled", "2560x1600\n2560x1600\n", Some(&REAL_EDID));
        put_connector(&fx, "card1-DP-1", "disconnected", "disabled", "", None);
        put_connector(&fx, "card1-HDMI-A-1", "disconnected", "disabled", "", None);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1, "only connected connectors");
        let c = &v[0];
        assert_eq!(c.id, "display:card1-eDP-1");
        assert_eq!(c.category, Category::Displays);
        assert_eq!(c.parent.as_deref(), Some("graphics:0000:00:02.0"));
        assert_eq!(c.title, "EF10QBC64.C");
        assert_eq!(spec(c, "Manufacturer").display(), "EDO");
        assert_eq!(spec(c, "Native resolution").display(), "2560 × 1600");
        assert_eq!(spec(c, "Refresh rate").display(), "165 Hz");
        assert_eq!(spec(c, "Screen size").display(), "14.9 inches (32 × 20 cm)");
        assert_eq!(spec(c, "Manufactured").display(), "Week 25 of 2024");
        assert_eq!(spec(c, "Input type").display(), "Digital (DisplayPort)");
        assert_eq!(spec(c, "Colour depth").display(), "10 bits per colour");
        assert_eq!(spec(c, "Connection").display(), "Internal panel (eDP)");
        assert_eq!(spec(c, "Active mode").display(), "2560x1600");
        assert_eq!(spec(c, "EDID version").display(), "1.4");
        assert!(spec(c, "HDR").display().contains("ST 2084"));
        assert_eq!(spec(c, "Serial number").display(), "Not reported");
        assert!(spec(c, "Serial number").identifier && spec(c, "Serial text").identifier);
        assert_eq!(spec(c, "Mode count").display(), "2");
        assert_eq!(c.subtitle.as_deref(), Some("14.9\" · 2560 × 1600 @ 165 Hz · eDP-1"));
    }

    #[test]
    fn known_vendor_serial_and_dp_mst_names() {
        let fx = Fx::new(&[]);
        card(&fx, "card1", "0000:00:02.0", "active");
        let dell = make_edid("DEL", 0xA0F8, 123456789, Some("DELL U2723QE"), Some("ABC123XYZ"), (60, 34));
        put_connector(&fx, "card1-DP-4", "connected", "enabled", "3840x2160\n1920x1080\n", Some(&dell));
        put_connector(&fx, "card1-DP-5", "connected", "disabled", "1920x1080\n", Some(&dell));
        put_connector(&fx, "card1-HDMI-A-1", "disconnected", "disabled", "", None);
        let v = collect_from(fx.path());
        assert_eq!(v.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["display:card1-DP-4", "display:card1-DP-5"]);
        let c = &v[0];
        assert_eq!(c.title, "DELL U2723QE", "model already starts with the vendor name");
        assert_eq!(spec(c, "Manufacturer").display(), "Dell (DEL)");
        assert_eq!(spec(c, "Serial number").display(), "123456789");
        assert_eq!(spec(c, "Serial text").display(), "ABC123XYZ");
        assert_eq!(spec(c, "Product code").display(), "0xA0F8");
        assert_eq!(spec(c, "Manufactured").display(), "Week 12 of 2020");
        assert_eq!(spec(c, "Native resolution").display(), "1920 × 1080");
        assert_eq!(spec(c, "Refresh rate").display(), "60 Hz");
        assert_eq!(spec(c, "Screen size").display(), "27.2 inches (60 × 34 cm)");
        assert_eq!(spec(c, "Connection").display(), "DisplayPort");
        assert_eq!(spec(c, "Active mode").display(), "3840x2160");
        assert_eq!(spec(c, "Colour formats").display(), "RGB 4:4:4, YCbCr 4:4:4");
        // Disabled connector: no guessed active mode.
        assert_eq!(spec(&v[1], "Active mode").display(), "Not reported");
        assert_eq!(spec(&v[1], "Enabled").display(), "No");
    }

    #[test]
    fn vendor_prefix_is_added_when_model_lacks_it() {
        let fx = Fx::new(&[]);
        let lg = make_edid("GSM", 1, 0, Some("27UK850"), None, (0, 0));
        put_connector(&fx, "card0-HDMI-A-1", "connected", "enabled", "3840x2160\n", Some(&lg));
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "LG Electronics 27UK850");
        assert_eq!(c.parent, None, "no device link -> no parent");
        assert_eq!(spec(c, "Screen size").display(), "Not reported");
        assert_eq!(spec(c, "Serial number").display(), "Not reported");
    }

    #[test]
    fn missing_edid_still_yields_a_component() {
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-Virtual-1", "connected", "enabled", "1280x800\n1024x768\n", None);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "Display on Virtual-1");
        assert_eq!(spec(c, "Model").display(), "Not reported");
        assert_eq!(spec(c, "Native resolution").display(), "Not reported");
        assert_eq!(spec(c, "Active mode").display(), "1280x800");
        assert_eq!(spec(c, "Connection").display(), "Virtual output");
        assert_eq!(spec(c, "Mode count").display(), "2");
    }

    #[test]
    fn unreadable_edid_reports_permission_required() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-DP-1", "connected", "enabled", "1920x1080\n", Some(&make_edid("DEL", 1, 0, None, None, (1, 1))));
        let p = fx.path().join("sys/class/drm/card0-DP-1/edid");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&p).is_ok() {
            return; // running as root: cannot provoke EACCES
        }
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Model").value.availability, Availability::PermissionRequired));
    }

    #[test]
    fn corrupt_and_truncated_edids_degrade_honestly() {
        // Short file
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-DP-1", "connected", "enabled", "1920x1080\n", Some(&REAL_EDID[..100]));
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Model").value.availability, Availability::Failed { .. }));
        assert!(spec(c, "Model").display().contains("100 bytes"));
        // Empty file
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-DP-1", "connected", "enabled", "1920x1080\n", Some(&[]));
        assert_eq!(spec(&collect_from(fx.path())[0], "Model").display(), "Not reported");
        // Bad header
        let mut bad = REAL_EDID.to_vec();
        bad[0] = 1;
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-DP-1", "connected", "enabled", "1920x1080\n", Some(&bad));
        assert!(spec(&collect_from(fx.path())[0], "Model").display().contains("header"));
        // Bad checksum: parsed, but flagged.
        let mut cs = REAL_EDID.to_vec();
        cs[127] ^= 0x55;
        let e = edid::parse(&cs).unwrap();
        assert!(!e.base_checksum_ok);
        let fx = Fx::new(&[]);
        put_connector(&fx, "card0-DP-1", "connected", "enabled", "1920x1080\n", Some(&cs));
        let c = &collect_from(fx.path())[0];
        assert!(spec(c, "EDID integrity").display().contains("checksum mismatch"));
        // Truncated extensions: base is fine, warning says so, nothing out of bounds.
        let e = edid::parse(&REAL_EDID[..200]).unwrap();
        assert!(e.warnings.iter().any(|w| w.contains("truncated")));
        assert_eq!(e.preferred, None, "the preferred timing lives in the missing DisplayID block");
        // Garbage never panics.
        for n in 0..=REAL_EDID.len() {
            let _ = edid::parse(&REAL_EDID[..n]);
        }
        let junk: Vec<u8> = (0..384u32).map(|i| (i * 37 % 251) as u8).collect();
        let _ = edid::parse(&junk);
        let mut hdr = vec![0xffu8; 384];
        hdr[..8].copy_from_slice(&[0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0]);
        hdr[126] = 3;
        let _ = edid::parse(&hdr);
    }

    #[test]
    fn suspended_gpu_connectors_are_never_opened() {
        let fx = Fx::new(&[]);
        card(&fx, "card0", "0000:01:00.0", "suspended");
        // If the collector opened these, the component would appear (status says connected).
        put_connector(&fx, "card0-HDMI-A-1", "connected", "enabled", "1920x1080\n", Some(&make_edid("DEL", 1, 5, Some("X"), None, (50, 30))));
        card(&fx, "card1", "0000:00:02.0", "active");
        put_connector(&fx, "card1-eDP-1", "connected", "enabled", "2560x1600\n", Some(&REAL_EDID));
        let ids: Vec<String> = collect_from(fx.path()).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["display:card1-eDP-1"]);
        // Same for other non-active states.
        for st in ["suspending", "resuming", "error"] {
            let fx = Fx::new(&[]);
            card(&fx, "card0", "0000:01:00.0", st);
            put_connector(&fx, "card0-HDMI-A-1", "connected", "enabled", "1920x1080\n", None);
            assert!(collect_from(fx.path()).is_empty(), "{st}");
        }
    }

    #[test]
    fn active_dgpu_connector_has_dgpu_parent() {
        let fx = Fx::new(&[]);
        card(&fx, "card0", "0000:01:00.0", "active");
        put_connector(&fx, "card0-HDMI-A-1", "connected", "enabled", "1920x1080\n", None);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.parent.as_deref(), Some("graphics:0000:01:00.0"));
        assert_eq!(spec(c, "Connection").display(), "HDMI");
    }

    #[test]
    fn vm_virtio_card_path_resolves_pci_parent() {
        let fx = Fx::new(&[]);
        fx.symlink("sys/class/drm/card0/device", "../../../devices/pci0000:00/0000:00:01.0/virtio0");
        put_connector(&fx, "card0-Virtual-1", "connected", "enabled", "1280x800\n", None);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.parent.as_deref(), Some("graphics:0000:00:01.0"));
    }

    #[test]
    fn nothing_without_drm() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
        let fx = Fx::new(&[("sys/class/drm/version", "drm 1.1.0\n"), ("sys/class/drm/renderD128/dev", "226:128\n"), ("sys/class/drm/card1/dev", "226:1\n")]);
        assert!(collect_from(fx.path()).is_empty());
    }

    #[test]
    fn diagonal_and_vendor_table() {
        assert!((edid::diagonal_inches(60, 34) - 27.1).abs() < 0.1);
        assert_eq!(edid::vendor_name("SDC"), Some("Samsung Display"));
        assert_eq!(edid::vendor_name("EDO"), None);
    }

    #[test]
    #[ignore = "reads the live machine; run with --ignored --nocapture"]
    fn print_live_displays() {
        for c in collect() {
            println!("== {} [{}] parent={:?} {:?}", c.title, c.id, c.parent, c.subtitle);
            for g in &c.groups {
                println!("  -- {}", g.title);
                for s in &g.specs {
                    println!("     {}: {}", s.label, if s.identifier { "(identifier) ".to_owned() + &s.display() } else { s.display() });
                }
            }
        }
    }
}
