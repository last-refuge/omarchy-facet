//! Linux inventory collectors. Read-only; never require privileges for browsing.
//!
//! Contract for every collector module: `collect() -> Vec<Component>` (reads `/`) and
//! `collect_from(root: &Path) -> Vec<Component>` (reads `<root>/sys`, `<root>/proc`, ... so tests
//! can use a fixture tree). Never panic, never block on missing hardware, never wake a device.

pub mod storage;
pub mod util;

use facet_core::Inventory;

/// Run every collector against the live system.
pub fn collect_all() -> Inventory {
    let mut components = Vec::new();
    components.extend(storage::collect());
    Inventory { components }
}
