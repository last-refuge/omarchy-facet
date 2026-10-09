use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use facet_core::{Field, StorageDevice, Transport};

const SYS_BLOCK: &str = "/sys/block";

/// Collect physical storage devices from sysfs.
pub fn collect() -> Vec<StorageDevice> {
    collect_from(Path::new(SYS_BLOCK))
}

/// Collect from an arbitrary sysfs-shaped root (used by fixture tests).
pub fn collect_from(root: &Path) -> Vec<StorageDevice> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| is_physical(n))
        .collect();
    names.sort();
    names.into_iter().map(|n| read_device(&root.join(&n), n)).collect()
}

/// Exclude device-mapper, zram, loop, ram and md: logical, not physical.
fn is_physical(name: &str) -> bool {
    !["dm-", "zram", "loop", "ram", "md", "sr", "fd"].iter().any(|p| name.starts_with(p))
}

fn read_attr(path: &Path) -> Field<String> {
    let source = format!("sysfs:{}", path.display());
    match fs::read_to_string(path) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() { Field::not_reported(source) } else { Field::available(t.to_owned(), source) }
        }
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Field::not_reported(source),
            ErrorKind::PermissionDenied => Field::permission_required(source),
            _ => Field::failed(e.to_string(), source),
        },
    }
}

fn map<T, U>(f: Field<T>, g: impl FnOnce(&T) -> Result<U, String>) -> Field<U> {
    use facet_core::Availability::*;
    let source = f.source;
    match f.availability {
        Available { value } => match g(&value) {
            Ok(v) => Field::available(v, source),
            Err(reason) => Field::failed(reason, source),
        },
        NotReported => Field::not_reported(source),
        PermissionRequired => Field::permission_required(source),
        Disconnected => Field { availability: Disconnected, source },
        Redacted => Field { availability: Redacted, source },
        Failed { reason } => Field::failed(reason, source),
    }
}

fn parse_flag(s: &String) -> Result<bool, String> {
    match s.as_str() {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(format!("unexpected flag value {other:?}")),
    }
}

fn read_device(dir: &Path, id: String) -> StorageDevice {
    let dev = dir.join("device");
    let transport = detect_transport(dir, &id);
    StorageDevice {
        model: read_attr(&dev.join("model")),
        vendor: read_attr(&dev.join("vendor")),
        serial: read_attr(&dev.join("serial")),
        firmware: {
            let f = read_attr(&dev.join("firmware_rev"));
            if f.value().is_some() { f } else { read_attr(&dev.join("rev")) }
        },
        transport,
        // sysfs `size` is always in 512-byte sectors regardless of device sector size.
        capacity_bytes: map(read_attr(&dir.join("size")), |s| {
            s.parse::<u64>().map(|n| n * 512).map_err(|e| e.to_string())
        }),
        rotational: map(read_attr(&dir.join("queue/rotational")), parse_flag),
        removable: map(read_attr(&dir.join("removable")), parse_flag),
        id,
    }
}

fn detect_transport(dir: &Path, id: &str) -> Field<Transport> {
    let source = format!("sysfs:{}", dir.display());
    let t = if id.starts_with("nvme") {
        Transport::Nvme
    } else if id.starts_with("mmcblk") {
        Transport::Mmc
    } else if id.starts_with("vd") {
        Transport::Virtio
    } else {
        // Resolve the device symlink and look for the bus in its path.
        let resolved: PathBuf = fs::canonicalize(dir.join("device")).unwrap_or_default();
        let p = resolved.to_string_lossy();
        if p.contains("/usb") {
            Transport::Usb
        } else if p.contains("/ata") {
            Transport::Sata
        } else {
            return Field::not_reported(source);
        }
    };
    Field::available(t, source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(files: &[(&str, &str)]) -> tempdir::Dir {
        tempdir::Dir::with(files)
    }

    mod tempdir {
        use std::{fs, path::{Path, PathBuf}};
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn with(files: &[(&str, &str)]) -> Self {
                let p = std::env::temp_dir().join(format!("facet-test-{}-{:?}", std::process::id(), std::thread::current().id()));
                let _ = fs::remove_dir_all(&p);
                for (rel, body) in files {
                    let f = p.join(rel);
                    fs::create_dir_all(f.parent().unwrap()).unwrap();
                    fs::write(f, body).unwrap();
                }
                Dir(p)
            }
            pub fn path(&self) -> &Path { &self.0 }
        }
        impl Drop for Dir { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    }

    #[test]
    fn parses_nvme_and_skips_logical() {
        let d = fixture(&[
            ("nvme0n1/device/model", "SAMSUNG TEST   \n"),
            ("nvme0n1/device/firmware_rev", "4L1QKXF7\n"),
            ("nvme0n1/size", "2000409264\n"),
            ("nvme0n1/queue/rotational", "0\n"),
            ("nvme0n1/removable", "0\n"),
            ("dm-0/size", "100\n"),
            ("zram0/size", "100\n"),
        ]);
        let devs = collect_from(d.path());
        assert_eq!(devs.len(), 1);
        let n = &devs[0];
        assert_eq!(n.model.value().map(String::as_str), Some("SAMSUNG TEST"));
        assert_eq!(n.capacity_bytes.value(), Some(&(2000409264u64 * 512)));
        assert_eq!(n.transport.value(), Some(&Transport::Nvme));
        assert!(n.serial.value().is_none(), "missing serial is not-reported, not empty");
    }

    #[test]
    fn malformed_values_fail_without_panicking() {
        let d = fixture(&[("sda/size", "abc\n"), ("sda/removable", "7\n")]);
        let devs = collect_from(d.path());
        assert!(matches!(devs[0].capacity_bytes.availability, facet_core::Availability::Failed { .. }));
        assert!(matches!(devs[0].removable.availability, facet_core::Availability::Failed { .. }));
    }

    #[test]
    fn missing_root_yields_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }
}
