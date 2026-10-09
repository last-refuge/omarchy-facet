use serde::{Deserialize, Serialize};

use crate::Field;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Nvme,
    Sata,
    Usb,
    Mmc,
    Virtio,
    Other,
}

/// A physical (or virtualised-as-physical) block device. Partitions and
/// device-mapper / zram devices are excluded by the collector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageDevice {
    /// Kernel name, e.g. `nvme0n1`.
    pub id: String,
    pub model: Field<String>,
    pub vendor: Field<String>,
    pub serial: Field<String>,
    pub firmware: Field<String>,
    pub transport: Field<Transport>,
    /// Capacity in bytes.
    pub capacity_bytes: Field<u64>,
    pub rotational: Field<bool>,
    pub removable: Field<bool>,
}
