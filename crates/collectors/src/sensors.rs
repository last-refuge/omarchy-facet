use std::path::Path;

use facet_core::Component;

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(_root: &Path) -> Vec<Component> {
    Vec::new()
}
