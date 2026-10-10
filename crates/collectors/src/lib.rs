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

use facet_core::Inventory;

/// Run every collector against the live system.
pub fn collect_all() -> Inventory {
    let mut components = Vec::new();
    components.extend(system::collect());
    components.extend(processor::collect());
    components.extend(memory::collect());
    components.extend(graphics::collect());
    components.extend(storage::collect());
    components.extend(network::collect());
    components.extend(displays::collect());
    components.extend(peripherals::collect());
    components.extend(power::collect());
    components.extend(sensors::collect());
    Inventory { components }
}
