//! Display units. Numbers are stored and exported in Celsius; only what people read converts.
//!
//! The current unit is a process-wide display setting so every place that formats a temperature (the
//! interface, search, text reports, the bar widget's JSON) agrees without threading a parameter through
//! every call. It defaults to Celsius, which keeps library users and tests deterministic; the apps apply
//! the user's preference at startup (see [`crate::Settings`]).

use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

impl TemperatureUnit {
    pub fn from_celsius(self, c: f64) -> f64 {
        match self {
            TemperatureUnit::Celsius => c,
            TemperatureUnit::Fahrenheit => c * 9.0 / 5.0 + 32.0,
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            TemperatureUnit::Celsius => "°C",
            TemperatureUnit::Fahrenheit => "°F",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TemperatureUnit::Celsius => "Celsius",
            TemperatureUnit::Fahrenheit => "Fahrenheit",
        }
    }

    /// Accepts `c`, `celsius`, `f`, `fahrenheit` in any case (and a leading degree sign).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().trim_start_matches('°').to_ascii_lowercase().as_str() {
            "c" | "celsius" => Some(TemperatureUnit::Celsius),
            "f" | "fahrenheit" => Some(TemperatureUnit::Fahrenheit),
            _ => None,
        }
    }

    /// `52 °C` / `126 °F`.
    pub fn format_celsius(self, c: f64) -> String {
        format!("{:.0} {}", self.from_celsius(c), self.symbol())
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Set the unit used by every temperature this process displays.
pub fn set_temperature_unit(unit: TemperatureUnit) {
    CURRENT.store(if unit == TemperatureUnit::Fahrenheit { 1 } else { 0 }, Ordering::Relaxed);
}

pub fn temperature_unit() -> TemperatureUnit {
    if CURRENT.load(Ordering::Relaxed) == 1 { TemperatureUnit::Fahrenheit } else { TemperatureUnit::Celsius }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_known_points() {
        let f = TemperatureUnit::Fahrenheit;
        assert_eq!(f.from_celsius(0.0), 32.0);
        assert_eq!(f.from_celsius(100.0), 212.0);
        assert_eq!(f.from_celsius(-40.0), -40.0);
        assert_eq!(TemperatureUnit::Celsius.from_celsius(52.0), 52.0);
    }

    #[test]
    fn formats_with_rounding() {
        assert_eq!(TemperatureUnit::Fahrenheit.format_celsius(52.0), "126 °F"); // 125.6
        assert_eq!(TemperatureUnit::Fahrenheit.format_celsius(43.85), "111 °F"); // 110.93
        assert_eq!(TemperatureUnit::Celsius.format_celsius(43.85), "44 °C");
        assert_eq!(TemperatureUnit::Fahrenheit.format_celsius(100.0), "212 °F");
    }

    #[test]
    fn parses_loosely_and_rejects_garbage() {
        for (s, u) in [("c", TemperatureUnit::Celsius), ("Celsius", TemperatureUnit::Celsius), ("°C", TemperatureUnit::Celsius), (" F ", TemperatureUnit::Fahrenheit), ("fahrenheit", TemperatureUnit::Fahrenheit)] {
            assert_eq!(TemperatureUnit::parse(s), Some(u), "{s:?}");
        }
        for s in ["", "kelvin", "cc", "5"] {
            assert_eq!(TemperatureUnit::parse(s), None, "{s:?}");
        }
    }
}
