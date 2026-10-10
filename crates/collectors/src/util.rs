//! Shared helpers. Every collector reads through a `root` so tests can point it at a fixture tree
//! mirroring the real filesystem (`<root>/sys/...`, `<root>/proc/...`); production uses `/`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use facet_core::Field;

/// Path of `rel` (no leading slash) under `root`.
pub fn at(root: &Path, rel: &str) -> PathBuf {
    root.join(rel.trim_start_matches('/'))
}

/// Provenance string for `rel`, always shown as the real path.
pub fn source(rel: &str) -> String {
    format!("sysfs:/{}", rel.trim_start_matches('/'))
}

/// Read a small text file. Missing -> not reported, EACCES -> permission required, empty -> not reported.
pub fn read_field(root: &Path, rel: &str) -> Field<String> {
    let src = source(rel);
    match fs::read_to_string(at(root, rel)) {
        Ok(s) => {
            let t = s.trim();
            if t.is_empty() { Field::not_reported(src) } else { Field::available(t.to_owned(), src) }
        }
        Err(e) => match e.kind() {
            ErrorKind::NotFound => Field::not_reported(src),
            ErrorKind::PermissionDenied => Field::permission_required(src),
            _ => Field::failed(e.to_string(), src),
        },
    }
}

/// Parse a field's text into another type; a parse failure becomes `Failed`, never a panic.
pub fn parse_field<T, U>(f: Field<T>, parse: impl FnOnce(&T) -> Result<U, String>) -> Field<U> {
    use facet_core::Availability::*;
    let source = f.source;
    match f.availability {
        Available { value } => match parse(&value) {
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

/// Sorted directory entry names (empty if the directory is missing or unreadable).
pub fn list_dir(root: &Path, rel: &str) -> Vec<String> {
    let Ok(rd) = fs::read_dir(at(root, rel)) else { return Vec::new() };
    let mut v: Vec<String> = rd.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

pub fn exists(root: &Path, rel: &str) -> bool {
    at(root, rel).exists()
}

/// Turn a field into a `Spec` of text.
pub fn text_spec(label: &str, f: Field<String>) -> facet_core::Spec {
    facet_core::Spec::new(label, parse_field(f, |s| Ok(facet_core::Value::Text(s.clone()))))
}

#[cfg(test)]
pub mod testutil {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    /// A throwaway fixture tree under the OS temp dir, deleted on drop.
    pub struct Fx(PathBuf);

    impl Fx {
        pub fn new(files: &[(&str, &str)]) -> Self {
            let p = std::env::temp_dir().join(format!("facet-fx-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            let fx = Fx(p);
            for (rel, body) in files {
                fx.write(rel, body);
            }
            fx
        }

        pub fn write(&self, rel: &str, body: &str) {
            let f = self.0.join(rel.trim_start_matches('/'));
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, body).unwrap();
        }

        pub fn write_bytes(&self, rel: &str, body: &[u8]) {
            let f = self.0.join(rel.trim_start_matches('/'));
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, body).unwrap();
        }

        pub fn symlink(&self, rel: &str, target: &str) {
            let f = self.0.join(rel.trim_start_matches('/'));
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, f).unwrap();
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Fx {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
