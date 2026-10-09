use serde::{Deserialize, Serialize};

use crate::StorageDevice;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub storage: Vec<StorageDevice>,
}

impl Report {
    pub fn new(storage: Vec<StorageDevice>) -> Self {
        Self { schema_version: SCHEMA_VERSION, storage }
    }

    /// Replace identifying values (serial numbers) with `Redacted`. Applied by default on export.
    pub fn redact_identifiers(&mut self) {
        for d in &mut self.storage {
            d.serial = crate::Field::redacted();
        }
    }
}
