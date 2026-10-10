//! User preferences shared by the app, the `facet` command and (through its JSON) the Omarchy bar widget.
//! Stored as JSON in `$XDG_CONFIG_HOME/facet/settings.json` (default `~/.config/facet/settings.json`).
//! A missing, unreadable or malformed file means "use the defaults"; it is never an error.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::units::{TemperatureUnit, set_temperature_unit};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub temperature_unit: TemperatureUnit,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { temperature_unit: TemperatureUnit::Fahrenheit }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("facet/settings.json"))
    }

    /// The saved settings, with `FACET_TEMPERATURE_UNIT` (c or f) overriding the unit for this run.
    pub fn load() -> Settings {
        let mut s = Self::path().map(|p| Self::load_from(&p)).unwrap_or_default();
        if let Some(u) = std::env::var("FACET_TEMPERATURE_UNIT").ok().and_then(|v| TemperatureUnit::parse(&v)) {
            s.temperature_unit = u;
        }
        s
    }

    pub fn load_from(path: &Path) -> Settings {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home or config directory"))?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        std::fs::write(path, json + "\n")
    }

    /// Make the process display temperatures in the saved unit.
    pub fn apply(&self) {
        set_temperature_unit(self.temperature_unit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("facet-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn defaults_to_fahrenheit() {
        assert_eq!(Settings::default().temperature_unit, TemperatureUnit::Fahrenheit);
        assert_eq!(Settings::load_from(Path::new("/nonexistent/facet/settings.json")).temperature_unit, TemperatureUnit::Fahrenheit);
    }

    #[test]
    fn round_trips_and_creates_the_directory() {
        let dir = tmp("rt");
        let path = dir.join("nested/settings.json");
        Settings { temperature_unit: TemperatureUnit::Celsius }.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path).temperature_unit, TemperatureUnit::Celsius);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn garbage_missing_fields_and_unknown_fields_fall_back_safely() {
        let dir = tmp("bad");
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [("empty", ""), ("junk", "not json {"), ("array", "[1,2]"), ("wrong-type", r#"{"temperature_unit": 5}"#), ("bad-variant", r#"{"temperature_unit": "kelvin"}"#)] {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            assert_eq!(Settings::load_from(&p), Settings::default(), "{name}");
        }
        let p = dir.join("extra");
        std::fs::write(&p, r#"{"temperature_unit": "celsius", "future_setting": true}"#).unwrap();
        assert_eq!(Settings::load_from(&p).temperature_unit, TemperatureUnit::Celsius, "unknown fields are ignored");
        let p = dir.join("missing-key");
        std::fs::write(&p, "{}").unwrap();
        assert_eq!(Settings::load_from(&p), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
