//! Linux inventory collectors. Read-only; never require privileges for browsing.
//!
//! Contract for every collector module: `collect() -> Vec<Component>` (reads `/`) and
//! `collect_from(root: &Path) -> Vec<Component>` (reads `<root>/sys`, `<root>/proc`, ... so tests
//! can use a fixture tree). Never panic, never block on missing hardware, never wake a device.

pub mod displays;
pub mod graphics;
pub mod memory;
pub mod network;
pub mod peripherals;
pub mod power;
pub mod processor;
pub mod sensors;
pub mod storage;
pub mod system;
pub mod util;

use facet_core::{Component, Inventory};

/// Every collector, fastest first. The UI runs each independently so slow ones fill in later.
pub type CollectFn = fn() -> Vec<Component>;

pub const COLLECTORS: &[(&str, CollectFn)] = &[
    ("system", system::collect),
    ("processor", processor::collect),
    ("memory", memory::collect),
    ("graphics", graphics::collect),
    ("storage", storage::collect),
    ("network", network::collect),
    ("displays", displays::collect),
    ("peripherals", peripherals::collect),
    ("power", power::collect),
    ("sensors", sensors::collect),
];

/// Run every collector against the live system, sequentially.
pub fn collect_all() -> Inventory {
    Inventory { components: COLLECTORS.iter().flat_map(|(_, f)| f()).collect() }
}
