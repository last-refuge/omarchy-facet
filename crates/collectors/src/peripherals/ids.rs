//! Parser for the pci.ids / usb.ids databases (same line format). Absence is normal: callers fall
//! back to hexadecimal ids rather than inventing names.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::util::at;

#[derive(Debug, Default)]
struct Vendor {
    name: String,
    devices: HashMap<u16, String>,
}

#[derive(Debug, Default)]
struct Class {
    name: String,
    subs: HashMap<u8, String>,
}

#[derive(Debug, Default)]
pub struct IdDb {
    vendors: HashMap<u16, Vendor>,
    classes: HashMap<u8, Class>,
    /// Real path of the database file, used as provenance.
    pub path: String,
}

enum Mode {
    None,
    Vendor(u16),
    Class(u8),
}

fn hex_prefix(line: &str, width: usize) -> Option<(u32, &str)> {
    let id = line.get(..width)?;
    if !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let rest = line.get(width..)?;
    Some((u32::from_str_radix(id, 16).ok()?, rest.trim()))
}

impl IdDb {
    pub fn pci(root: &Path) -> Option<IdDb> {
        Self::load(root, &["usr/share/hwdata/pci.ids", "usr/share/misc/pci.ids", "usr/share/pci.ids"])
    }

    pub fn usb(root: &Path) -> Option<IdDb> {
        Self::load(root, &["usr/share/hwdata/usb.ids", "usr/share/misc/usb.ids", "usr/share/usb.ids"])
    }

    pub fn load(root: &Path, candidates: &[&str]) -> Option<IdDb> {
        for rel in candidates {
            if let Ok(bytes) = fs::read(at(root, rel)) {
                return Some(Self::parse(&String::from_utf8_lossy(&bytes), &format!("/{rel}")));
            }
        }
        None
    }

    pub fn parse(text: &str, path: &str) -> IdDb {
        let mut db = IdDb { path: path.to_owned(), ..Default::default() };
        let mut mode = Mode::None;
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("C ") {
                mode = match hex_prefix(rest, 2) {
                    Some((id, name)) => {
                        db.classes.entry(id as u8).or_default().name = name.to_owned();
                        Mode::Class(id as u8)
                    }
                    None => Mode::None,
                };
            } else if let Some(rest) = line.strip_prefix('\t') {
                if rest.starts_with('\t') {
                    continue; // subsystem / programming-interface level: not needed
                }
                match mode {
                    Mode::Vendor(v) => {
                        if let (Some((id, name)), Some(ven)) = (hex_prefix(rest, 4), db.vendors.get_mut(&v)) {
                            ven.devices.insert(id as u16, name.to_owned());
                        }
                    }
                    Mode::Class(c) => {
                        if let (Some((id, name)), Some(cl)) = (hex_prefix(rest, 2), db.classes.get_mut(&c)) {
                            cl.subs.insert(id as u8, name.to_owned());
                        }
                    }
                    Mode::None => {}
                }
            } else if line.len() > 6 && line.as_bytes()[4] == b' ' && line.as_bytes()[5] == b' ' {
                mode = match hex_prefix(line, 4) {
                    Some((id, name)) => {
                        db.vendors.insert(id as u16, Vendor { name: name.to_owned(), devices: HashMap::new() });
                        Mode::Vendor(id as u16)
                    }
                    None => Mode::None,
                };
            } else {
                mode = Mode::None; // other sections (AT, HID, HUT, ...)
            }
        }
        db
    }

    pub fn vendor(&self, v: u16) -> Option<&str> {
        self.vendors.get(&v).map(|x| x.name.as_str())
    }

    pub fn device(&self, v: u16, d: u16) -> Option<&str> {
        self.vendors.get(&v)?.devices.get(&d).map(String::as_str)
    }

    pub fn class(&self, c: u8) -> Option<&str> {
        self.classes.get(&c).map(|x| x.name.as_str())
    }

    pub fn subclass(&self, c: u8, s: u8) -> Option<&str> {
        self.classes.get(&c)?.subs.get(&s).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment\n8086  Intel Corporation\n\t1234  Fancy Controller\n\t\t17aa 0001  Subsystem thing\n14c3  MediaTek Inc.\n\t7925  MT7925 Wi-Fi 7\nAT 0001  junk\nC 0c  Serial bus controller\n\t03  USB controller\n\t\t30  XHCI\nC 02  Network controller\n\t00  Ethernet controller\n";

    #[test]
    fn parses_vendors_devices_classes() {
        let db = IdDb::parse(SAMPLE, "/x");
        assert_eq!(db.vendor(0x8086), Some("Intel Corporation"));
        assert_eq!(db.device(0x14c3, 0x7925), Some("MT7925 Wi-Fi 7"));
        assert_eq!(db.device(0x8086, 0x7925), None);
        assert_eq!(db.class(0x0c), Some("Serial bus controller"));
        assert_eq!(db.subclass(0x0c, 0x03), Some("USB controller"));
        assert_eq!(db.subclass(0x02, 0x00), Some("Ethernet controller"));
        assert_eq!(db.vendor(0x0001), None);
    }

    #[test]
    fn garbage_does_not_panic() {
        let db = IdDb::parse("\u{fffd}\u{fffd}\n\t\t\t\nzzzz  nope\n12\n\t12\n", "/x");
        assert_eq!(db.vendor(0), None);
    }
}
