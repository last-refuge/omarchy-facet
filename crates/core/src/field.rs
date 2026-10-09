use serde::{Deserialize, Serialize};

/// Why a value is, or is not, present. Never collapse these into a bare `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Availability<T> {
    Available { value: T },
    NotReported,
    PermissionRequired,
    Disconnected,
    /// Present but withheld from this report (e.g. serial numbers by default).
    Redacted,
    Failed { reason: String },
}

/// A single datum with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field<T> {
    #[serde(flatten)]
    pub availability: Availability<T>,
    /// Where the value came from, e.g. `sysfs:/sys/block/nvme0n1/device/model`.
    pub source: String,
}

impl<T> Field<T> {
    pub fn available(value: T, source: impl Into<String>) -> Self {
        Self { availability: Availability::Available { value }, source: source.into() }
    }

    pub fn not_reported(source: impl Into<String>) -> Self {
        Self { availability: Availability::NotReported, source: source.into() }
    }

    pub fn redacted() -> Self {
        Self { availability: Availability::Redacted, source: "redacted".into() }
    }

    pub fn permission_required(source: impl Into<String>) -> Self {
        Self { availability: Availability::PermissionRequired, source: source.into() }
    }

    pub fn failed(reason: impl Into<String>, source: impl Into<String>) -> Self {
        Self { availability: Availability::Failed { reason: reason.into() }, source: source.into() }
    }

    pub fn value(&self) -> Option<&T> {
        match &self.availability {
            Availability::Available { value } => Some(value),
            _ => None,
        }
    }
}
