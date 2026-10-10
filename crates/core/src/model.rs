//! Presentation model: what the UI, search and exports consume. Typed per-category records convert
//! into [`Component`]s; values keep their units so formatting and export stay consistent.

use serde::{Deserialize, Serialize};

use crate::{Availability, Field, format_bytes};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    System,
    Processor,
    Memory,
    Graphics,
    Storage,
    Network,
    Displays,
    Peripherals,
    Power,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::System,
        Category::Processor,
        Category::Memory,
        Category::Graphics,
        Category::Storage,
        Category::Network,
        Category::Displays,
        Category::Peripherals,
        Category::Power,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::System => "System",
            Category::Processor => "Processor",
            Category::Memory => "Memory",
            Category::Graphics => "Graphics",
            Category::Storage => "Storage",
            Category::Network => "Network",
            Category::Displays => "Displays",
            Category::Peripherals => "Peripherals",
            Category::Power => "Power & sensors",
        }
    }
}

/// A measured or reported value with its unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", content = "v", rename_all = "snake_case")]
pub enum Value {
    Text(String),
    /// Decimal capacity as drives are labelled (1 TB = 10^12 B).
    Bytes(u64),
    /// Binary size as memory and caches are labelled (1 GiB = 2^30 B).
    BinaryBytes(u64),
    Hertz(u64),
    Celsius(f64),
    Percent(f64),
    Count(u64),
    Bool(bool),
    Watts(f64),
    Volts(f64),
    Rpm(u64),
    /// Bits per second (link speeds).
    Bps(u64),
}

impl Value {
    /// Numeric magnitude for trend charts; `None` for text and booleans.
    pub fn as_f64(&self) -> Option<f64> {
        Some(match self {
            Value::Text(_) | Value::Bool(_) => return None,
            Value::Bytes(v) | Value::BinaryBytes(v) | Value::Hertz(v) | Value::Count(v) | Value::Rpm(v) | Value::Bps(v) => *v as f64,
            Value::Celsius(v) | Value::Percent(v) | Value::Watts(v) | Value::Volts(v) => *v,
        })
    }

    pub fn display(&self) -> String {
        match self {
            Value::Text(s) => s.clone(),
            Value::Bytes(b) => format_bytes(*b),
            Value::BinaryBytes(b) => format_binary(*b),
            Value::Hertz(h) => {
                if *h >= 1_000_000_000 {
                    format!("{:.2} GHz", *h as f64 / 1e9)
                } else if *h >= 1_000_000 {
                    format!("{} MHz", trim(*h as f64 / 1e6))
                } else if *h >= 1_000 {
                    format!("{} kHz", trim(*h as f64 / 1e3))
                } else {
                    format!("{h} Hz")
                }
            }
            Value::Celsius(c) => format!("{c:.0} °C"),
            Value::Percent(p) => format!("{p:.0}%"),
            Value::Count(n) => n.to_string(),
            Value::Bool(b) => if *b { "Yes".into() } else { "No".into() },
            Value::Watts(w) => format!("{w:.1} W"),
            Value::Volts(v) => format!("{v:.2} V"),
            Value::Rpm(r) => format!("{r} RPM"),
            Value::Bps(b) => {
                if *b >= 1_000_000_000 { format!("{} Gb/s", trim(*b as f64 / 1e9)) } else { format!("{} Mb/s", trim(*b as f64 / 1e6)) }
            }
        }
    }
}

fn format_binary(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{bytes} B") } else { format!("{} {}", trim(v), UNITS[i]) }
}

fn trim(x: f64) -> String {
    let s = format!("{x:.1}");
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// One labelled datum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spec {
    pub label: String,
    pub value: Field<Value>,
    /// Identifier values (serial numbers, MAC addresses) are redacted unless explicitly included.
    #[serde(default)]
    pub identifier: bool,
    /// Live measurements (temperatures, usage) are refreshed; static specs are cached.
    #[serde(default)]
    pub live: bool,
    /// Rated capability (e.g. link supports 16 GT/s) as opposed to the current operating value.
    #[serde(default)]
    pub rated: bool,
}

impl Spec {
    pub fn new(label: impl Into<String>, value: Field<Value>) -> Self {
        Self { label: label.into(), value, identifier: false, live: false, rated: false }
    }

    pub fn text(label: &str, v: impl Into<String>, source: &str) -> Self {
        Self::new(label, Field::available(Value::Text(v.into()), source))
    }

    pub fn val(label: &str, v: Value, source: &str) -> Self {
        Self::new(label, Field::available(v, source))
    }

    pub fn missing(label: &str, source: &str) -> Self {
        Self::new(label, Field::not_reported(source))
    }

    pub fn identifier(mut self) -> Self {
        self.identifier = true;
        self
    }

    pub fn live(mut self) -> Self {
        self.live = true;
        self
    }

    pub fn rated(mut self) -> Self {
        self.rated = true;
        self
    }

    /// Text shown to people. Never empty: absent states are spelled out.
    pub fn display(&self) -> String {
        match &self.value.availability {
            Availability::Available { value } => value.display(),
            Availability::NotReported => "Not reported".into(),
            Availability::PermissionRequired => "Permission required".into(),
            Availability::Disconnected => "Disconnected".into(),
            Availability::Redacted => "Hidden".into(),
            Availability::Failed { reason } => format!("Unavailable ({reason})"),
        }
    }

    pub fn is_available(&self) -> bool {
        self.value.value().is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecGroup {
    pub title: String,
    pub specs: Vec<Spec>,
    /// Shown collapsed under "Technical details".
    #[serde(default)]
    pub technical: bool,
}

impl SpecGroup {
    pub fn new(title: &str, specs: Vec<Spec>) -> Self {
        Self { title: title.into(), specs, technical: false }
    }

    pub fn technical(mut self) -> Self {
        self.technical = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Component {
    /// Stable across refreshes, e.g. `storage:nvme0n1`.
    pub id: String,
    pub category: Category,
    pub title: String,
    pub subtitle: Option<String>,
    /// Parent in the connection graph (device behind a hub, drive behind a controller).
    pub parent: Option<String>,
    /// Short facts for cards and rows.
    pub summary: Vec<Spec>,
    pub groups: Vec<SpecGroup>,
}

impl Component {
    pub fn new(id: &str, category: Category, title: &str) -> Self {
        Self { id: id.into(), category, title: title.into(), subtitle: None, parent: None, summary: vec![], groups: vec![] }
    }

    pub fn subtitle(mut self, s: &str) -> Self {
        self.subtitle = Some(s.into());
        self
    }

    pub fn parent(mut self, id: &str) -> Self {
        self.parent = Some(id.into());
        self
    }

    pub fn summary(mut self, specs: Vec<Spec>) -> Self {
        self.summary = specs;
        self
    }

    pub fn group(mut self, g: SpecGroup) -> Self {
        self.groups.push(g);
        self
    }

    pub fn all_specs(&self) -> impl Iterator<Item = &Spec> {
        self.summary.iter().chain(self.groups.iter().flat_map(|g| g.specs.iter()))
    }

    /// Case-insensitive match over title, subtitle, id, and every label and displayed value.
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        q.split_whitespace().all(|word| {
            self.title.to_lowercase().contains(word)
                || self.subtitle.as_deref().is_some_and(|s| s.to_lowercase().contains(word))
                || self.id.to_lowercase().contains(word)
                || self.category.label().to_lowercase().contains(word)
                || self.all_specs().any(|s| !s.identifier && (s.label.to_lowercase().contains(word) || s.display().to_lowercase().contains(word)))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub components: Vec<Component>,
}

impl Inventory {
    pub fn in_category(&self, c: Category) -> impl Iterator<Item = &Component> {
        self.components.iter().filter(move |x| x.category == c)
    }

    pub fn get(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }

    pub fn search(&self, query: &str) -> Vec<&Component> {
        self.components.iter().filter(|c| c.matches(query)).collect()
    }

    pub fn children_of<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Component> {
        self.components.iter().filter(move |c| c.parent.as_deref() == Some(id))
    }

    /// Replace identifier values with `Redacted`.
    pub fn redact_identifiers(&mut self) {
        for c in &mut self.components {
            for s in c.summary.iter_mut().chain(c.groups.iter_mut().flat_map(|g| g.specs.iter_mut())) {
                if s.identifier {
                    s.value = Field::redacted();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Component {
        Component::new("storage:nvme0n1", Category::Storage, "SAMSUNG MZAL81T0HFLB")
            .summary(vec![Spec::val("Capacity", Value::Bytes(1_024_209_543_168), "t")])
            .group(SpecGroup::new("Identity", vec![Spec::text("Serial", "S7R9", "t").identifier()]))
    }

    #[test]
    fn displays_units() {
        assert_eq!(Value::Hertz(4_700_000_000).display(), "4.70 GHz");
        assert_eq!(Value::Hertz(800_000_000).display(), "800 MHz");
        assert_eq!(Value::Bps(2_500_000_000).display(), "2.5 Gb/s");
        assert_eq!(Value::Bps(1_000_000_000).display(), "1 Gb/s");
        assert_eq!(Value::Celsius(43.4).display(), "43 °C");
        assert_eq!(Value::Hertz(165).display(), "165 Hz");
        assert_eq!(Value::Hertz(59_940_000).display(), "59.9 MHz");
        assert_eq!(Value::BinaryBytes(48 * 1024).display(), "48 KiB");
        assert_eq!(Value::BinaryBytes(36864 * 1024).display(), "36 MiB");
        assert_eq!(Value::BinaryBytes(32 * 1024 * 1024 * 1024).display(), "32 GiB");
        assert_eq!(Value::BinaryBytes(32_567_160 * 1024).display(), "31.1 GiB");
    }

    #[test]
    fn search_matches_plain_language_and_skips_identifiers() {
        let c = sample();
        assert!(c.matches("samsung"));
        assert!(c.matches("storage 1.0 tb"));
        assert!(!c.matches("S7R9"), "serials are not searchable");
        assert!(!c.matches("nonexistent"));
    }

    #[test]
    fn redaction_hits_every_identifier() {
        let mut inv = Inventory { components: vec![sample()] };
        inv.redact_identifiers();
        let s = &inv.components[0].groups[0].specs[0];
        assert_eq!(s.display(), "Hidden");
    }
}
