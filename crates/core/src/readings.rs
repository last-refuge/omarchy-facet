//! The handful of live numbers worth showing at a glance (overview page, bar widget, `facet --status`).
//!
//! One implementation so every surface agrees. Absent values are explained, never shown as zero, and
//! a GPU that is asleep is reported as asleep rather than queried.

use serde::{Deserialize, Serialize};

use crate::{Category, Component, Inventory, Spec, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// Stable machine name: `cpu_temperature`, `memory_in_use`, `gpu_temperature`, `battery`, `storage_temperature`.
    pub id: String,
    pub label: String,
    /// Ready to show, in the user's temperature unit: `126 °F`, `28%`, or `—` when the value is unavailable.
    pub display: String,
    /// Numeric value when there is one: always degrees Celsius for temperatures (whatever unit is displayed),
    /// or percent.
    pub value: Option<f64>,
    /// Plain-language context: the sensor, the status, or why the value is absent.
    pub note: String,
    pub available: bool,
    /// Extra figure for the reading, e.g. memory as `7.5 / 31.1 GiB`. Empty when there is none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Where a live number comes from (component id and spec label), so a UI can chart its history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<(String, String)>,
}

impl Reading {
    fn ok(id: &str, label: &str, v: &Value, note: impl Into<String>) -> Self {
        Reading { id: id.into(), label: label.into(), display: v.display(), value: v.as_f64(), note: note.into(), available: true, detail: String::new(), source: None }
    }

    fn from(mut self, component: &str, spec: &str) -> Self {
        self.source = Some((component.to_owned(), spec.to_owned()));
        self
    }

    fn absent(id: &str, label: &str, note: impl Into<String>) -> Self {
        Reading { id: id.into(), label: label.into(), display: "—".into(), value: None, note: note.into(), available: false, detail: String::new(), source: None }
    }
}

fn find_live<'a>(inv: &'a Inventory, pred: &dyn Fn(&Component, &Spec) -> bool) -> Option<(&'a Component, &'a Spec)> {
    inv.components.iter().find_map(|c| c.all_specs().find(|s| s.live && pred(c, s)).map(|s| (c, s)))
}

fn is_celsius(s: &Spec) -> bool {
    matches!(s.value.value(), Some(Value::Celsius(_)))
}

fn bytes_of(c: &Component, label: &str) -> Option<u64> {
    c.all_specs().find(|s| s.label == label).and_then(|s| match s.value.value() {
        Some(Value::BinaryBytes(b) | Value::Bytes(b)) => Some(*b),
        _ => None,
    })
}

/// `7.5 / 31.1 GiB`: both figures in the unit that suits the total.
fn used_of_total(used: u64, total: u64) -> String {
    const GIB: f64 = (1u64 << 30) as f64;
    const MIB: f64 = (1u64 << 20) as f64;
    let (unit, div) = if total as f64 >= GIB { ("GiB", GIB) } else { ("MiB", MIB) };
    format!("{:.1} / {:.1} {unit}", used as f64 / div, total as f64 / div)
}

/// Up to five readings, in display order. Readings with no source on this machine are omitted
/// (a desktop has no battery); readings that exist but are unavailable say why.
pub fn headline_readings(inv: &Inventory) -> Vec<Reading> {
    let mut out = Vec::new();

    if let Some((c, s)) = find_live(inv, &|c, s| c.id.starts_with("sensors:") && is_celsius(s) && (s.label.contains("Package") || s.label.contains("Tctl") || s.label.contains("Tdie"))) {
        out.push(Reading::ok("cpu_temperature", "CPU temperature", s.value.value().unwrap(), "Package sensor").from(&c.id, &s.label));
    } else if let Some((c, s)) = find_live(inv, &|c, s| c.category == Category::Processor && is_celsius(s)) {
        out.push(Reading::ok("cpu_temperature", "CPU temperature", s.value.value().unwrap(), c.title.clone()).from(&c.id, &s.label));
    }

    if let Some(mem) = inv.get("memory:system") {
        let total = bytes_of(mem, "Usable memory").or_else(|| bytes_of(mem, "Installed"));
        let pct = find_live(inv, &|c, s| c.id == "memory:system" && s.label == "In use")
            .and_then(|(_, s)| s.value.value().and_then(Value::as_f64))
            .or_else(|| match (total, bytes_of(mem, "Available memory")) {
                (Some(t), Some(a)) if t > 0 && a <= t => Some(100.0 * (t - a) as f64 / t as f64),
                _ => None,
            });
        if let Some(pct) = pct {
            let mut r = Reading::ok("memory_in_use", "Memory in use", &Value::Percent(pct), "Of usable memory");
            if let Some(t) = total.filter(|t| *t > 0) {
                r.detail = used_of_total((t as f64 * pct / 100.0) as u64, t);
            }
            out.push(r);
        }
    }
    let gpus: Vec<(&Component, &Spec)> =
        inv.in_category(Category::Graphics).filter_map(|g| g.all_specs().find(|s| s.label == "Temperature").map(|s| (g, s))).collect();
    if let Some((g, s)) = gpus.iter().find(|(_, s)| s.is_available()) {
        out.push(Reading::ok("gpu_temperature", "GPU temperature", s.value.value().unwrap(), g.title.clone()).from(&g.id, &s.label));
    } else if let Some((g, s)) = gpus.iter().find(|(_, s)| s.value.source.contains("suspended")).or(gpus.first()) {
        let note = if s.value.source.contains("suspended") {
            format!("{} is asleep; not queried so it stays asleep", g.title)
        } else {
            format!("{}: the driver doesn't report it", g.title)
        };
        out.push(Reading::absent("gpu_temperature", "GPU temperature", note));
    }

    if let Some(b) = inv.components.iter().find(|c| c.id.starts_with("power:BAT"))
        && let Some(charge) = b.all_specs().find(|s| s.label == "Charge" && s.is_available())
    {
        let status = b.all_specs().find(|s| s.label == "Status").map(Spec::display).unwrap_or_default();
        out.push(Reading::ok("battery", "Battery", charge.value.value().unwrap(), status).from(&b.id, &charge.label));
    }

    if let Some((c, s)) = find_live(inv, &|c, s| c.id.starts_with("sensors:nvme") && is_celsius(s) && s.label == "Composite") {
        let _ = c;
        out.push(Reading::ok("storage_temperature", "Drive temperature", s.value.value().unwrap(), "NVMe composite sensor"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn ids(v: &[Reading]) -> Vec<&str> {
        v.iter().map(|r| r.id.as_str()).collect()
    }

    #[test]
    fn laptop_fixture_has_cpu_memory_gpu_and_battery() {
        let r = headline_readings(&fixtures::laptop());
        assert_eq!(ids(&r), ["cpu_temperature", "memory_in_use", "gpu_temperature", "battery"]);
        let cpu = &r[0];
        assert_eq!((cpu.display.as_str(), cpu.available), ("52 °C", true));
        assert_eq!(cpu.value, Some(52.0));
        // The first GPU with a reading wins; the asleep discrete GPU is not asked.
        assert_eq!(r[2].display, "48 °C");
        assert_eq!(r[3].display, "86%");
        assert_eq!(r[3].note, "Discharging");
        assert_eq!(r[0].source, Some(("sensors:coretemp".to_owned(), "Package".to_owned())));
        assert_eq!(r[1].detail, "12.2 / 32.0 GiB");
    }

    #[test]
    fn desktop_has_no_battery_and_vm_has_no_sensors() {
        assert!(!ids(&headline_readings(&fixtures::desktop())).contains(&"battery"));
        let vm = headline_readings(&fixtures::vm());
        assert!(!ids(&vm).contains(&"cpu_temperature"), "no sensors: omitted, not zero");
        assert!(vm.iter().all(|r| r.available || r.display == "—"));
    }

    #[test]
    fn sleeping_gpu_is_explained_not_zeroed() {
        let mut inv = fixtures::laptop();
        inv.components.retain(|c| c.id != "graphics:0000:00:02.0"); // leave only the suspended dGPU
        let r = headline_readings(&inv);
        let gpu = r.iter().find(|r| r.id == "gpu_temperature").unwrap();
        assert!(!gpu.available);
        assert_eq!(gpu.display, "—");
        assert!(gpu.note.contains("asleep"));
        assert_eq!(gpu.value, None);
    }

    #[test]
    fn empty_inventory_yields_nothing() {
        assert!(headline_readings(&Inventory::default()).is_empty());
    }
}
