use serde::{Deserialize, Serialize};

use crate::{Availability, Inventory};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub generator: String,
    pub inventory: Inventory,
}

impl Report {
    pub fn new(mut inventory: Inventory, include_identifiers: bool) -> Self {
        if !include_identifiers {
            inventory.redact_identifiers();
        }
        Self { schema_version: SCHEMA_VERSION, generator: format!("facet {}", env!("CARGO_PKG_VERSION")), inventory }
    }

    /// Plain-text rendering for pasting into bug reports.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for cat in crate::Category::ALL {
            let comps: Vec<_> = self.inventory.in_category(cat).collect();
            if comps.is_empty() {
                continue;
            }
            out.push_str(&format!("== {} ==\n", cat.label()));
            for c in comps {
                out.push_str(&format!("{}{}\n", c.title, c.subtitle.as_deref().map(|s| format!("  ({s})")).unwrap_or_default()));
                for s in &c.summary {
                    out.push_str(&format!("  {:<22} {}\n", s.label, s.display()));
                }
                for g in &c.groups {
                    out.push_str(&format!("  [{}]\n", g.title));
                    for s in &g.specs {
                        let note = match &s.value.availability {
                            Availability::Available { .. } => String::new(),
                            _ => String::new(),
                        };
                        out.push_str(&format!("    {:<20} {}{}\n", s.label, s.display(), note));
                    }
                }
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Category, Component, Spec, SpecGroup};

    #[test]
    fn redacts_by_default_and_includes_on_request() {
        let c = Component::new("x:1", Category::Storage, "Disk")
            .group(SpecGroup::new("Identity", vec![Spec::text("Serial", "ABC123", "t").identifier()]));
        let inv = Inventory { components: vec![c] };
        let hidden = Report::new(inv.clone(), false);
        assert!(!hidden.to_text().contains("ABC123"));
        assert!(Report::new(inv, true).to_text().contains("ABC123"));
    }
}
