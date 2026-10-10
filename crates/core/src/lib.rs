//! Typed hardware records, availability states and the report schema.

pub mod field;
pub mod format;
pub mod model;
pub mod report;

pub use format::format_bytes;
pub use model::{Category, Component, Inventory, Spec, SpecGroup, Value};
pub use field::{Availability, Field};
pub use report::{Report, SCHEMA_VERSION};
