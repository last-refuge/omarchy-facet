//! Minimal pci.ids lookup (vendor, device, subsystem). The database is plain text; we scan it once per
//! lookup and stop as soon as the vendor section ends. Absent database -> `None`, never a guess.

use std::fs;
use std::path::Path;

use crate::util::at;

pub const DB_PATHS: [&str; 2] = ["usr/share/hwdata/pci.ids", "usr/share/misc/pci.ids"];

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Names {
    pub vendor: Option<String>,
    pub device: Option<String>,
    pub subsystem: Option<String>,
    /// Path (relative to root) of the database that was read, if any.
    pub db: Option<&'static str>,
}

/// Look up names. All ids are lowercase 4-digit hex without prefix.
pub fn lookup(root: &Path, vendor: &str, device: &str, sub: Option<(&str, &str)>) -> Names {
    for p in DB_PATHS {
        if let Ok(text) = fs::read_to_string(at(root, p)) {
            let mut n = lookup_in(&text, vendor, device, sub);
            n.db = Some(p);
            return n;
        }
    }
    Names::default()
}

pub fn lookup_in(text: &str, vendor: &str, device: &str, sub: Option<(&str, &str)>) -> Names {
    let mut out = Names::default();
    let mut in_vendor = false;
    let mut in_device = false;
    let mut sub_vendor_name: Option<String> = None;
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("\t\t") {
            if in_device
                && let (Some((sv, sd)), Some((id, name))) = (sub, split_id(rest, 9))
                && id.eq_ignore_ascii_case(&format!("{sv} {sd}"))
            {
                out.subsystem = Some(name.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix('\t') {
            if in_vendor {
                in_device = false;
                if let Some((id, name)) = split_id(rest, 4)
                    && id.eq_ignore_ascii_case(device)
                {
                    out.device = Some(name.to_owned());
                    in_device = true;
                }
            }
        } else if line.starts_with('C') && line.as_bytes().get(1) == Some(&b' ') {
            break; // class section: vendors are done
        } else if let Some((id, name)) = split_id(line, 4) {
            if in_vendor {
                in_vendor = false;
                in_device = false;
            }
            if id.eq_ignore_ascii_case(vendor) {
                out.vendor = Some(name.to_owned());
                in_vendor = true;
            }
            if let Some((sv, _)) = sub
                && id.eq_ignore_ascii_case(sv)
            {
                sub_vendor_name = Some(name.to_owned());
            }
        }
    }
    // pci.ids lists named subsystems only; fall back to the subsystem vendor's name (as lspci does).
    if out.subsystem.is_none()
        && let (Some(v), Some((sv, sd))) = (sub_vendor_name, sub)
    {
        out.subsystem = Some(format!("{v} device [{sv}:{sd}]"));
    }
    out
}

/// Split "<id>  <name>" where the id is `len` hex-ish characters.
fn split_id(s: &str, len: usize) -> Option<(&str, &str)> {
    let id = s.get(..len)?;
    if !id.chars().all(|c| c.is_ascii_hexdigit() || c == ' ') {
        return None;
    }
    let name = s.get(len..)?.trim();
    if name.is_empty() { None } else { Some((id, name)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DB: &str = "# comment\n10de  NVIDIA Corporation\n\t2d18  GB206M [GeForce RTX 5070 Max-Q / Mobile]\n\t\t17aa 3e31  Special\n17aa  Lenovo\n\t3e31  Not a device of 17aa\n8086  Intel Corporation\n\ta788  Raptor Lake-S UHD Graphics\nC 03  Display controller\n";

    #[test]
    fn finds_vendor_device_and_subsystem_vendor() {
        let n = lookup_in(DB, "8086", "a788", Some(("17aa", "3e31")));
        assert_eq!(n.vendor.as_deref(), Some("Intel Corporation"));
        assert_eq!(n.device.as_deref(), Some("Raptor Lake-S UHD Graphics"));
        assert_eq!(n.subsystem.as_deref(), Some("Lenovo device [17aa:3e31]"));
    }

    #[test]
    fn named_subsystem_wins() {
        let n = lookup_in(DB, "10de", "2d18", Some(("17aa", "3e31")));
        assert_eq!(n.subsystem.as_deref(), Some("Special"));
        assert!(n.device.unwrap().contains("RTX 5070"));
    }

    #[test]
    fn unknown_ids_are_none() {
        let n = lookup_in(DB, "dead", "beef", None);
        assert_eq!(n, Names::default());
    }
}
