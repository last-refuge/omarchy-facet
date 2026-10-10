//! Typed hardware records, availability states and the report schema.

pub mod field;
pub mod fixtures;
pub mod format;
pub mod model;
pub mod readings;
pub mod report;

pub use format::format_bytes;
pub use model::{Category, Component, Inventory, Spec, SpecGroup, Value};
pub use field::{Availability, Field};
pub use readings::{Reading, headline_readings};
pub use report::{Report, SCHEMA_VERSION};
