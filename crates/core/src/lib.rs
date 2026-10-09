//! Typed hardware records, availability states and the report schema.

pub mod field;
pub mod format;
pub mod report;
pub mod storage;

pub use format::format_bytes;
pub use field::{Availability, Field};
pub use report::{Report, SCHEMA_VERSION};
pub use storage::{StorageDevice, Transport};
