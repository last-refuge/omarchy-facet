//! The unit is process-wide, so this lives in its own test binary (its own process) and runs as a single
//! test: nothing else here can observe it half-changed.

use facet_core::{Settings, TemperatureUnit, Value, set_temperature_unit, temperature_unit};

#[test]
fn display_follows_the_current_unit_and_the_stored_number_never_changes() {
    assert_eq!(temperature_unit(), TemperatureUnit::Celsius, "library default is Celsius");
    let v = Value::Celsius(52.0);
    assert_eq!(v.display(), "52 °C");

    set_temperature_unit(TemperatureUnit::Fahrenheit);
    assert_eq!(v.display(), "126 °F");
    assert_eq!(v.as_f64(), Some(52.0), "the number stays Celsius for charts and JSON");
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("celsius") && json.contains("52"), "exports stay in Celsius: {json}");
    assert_eq!(Value::Percent(38.0).display(), "38%", "other units are unaffected");

    // Applying settings switches the process.
    set_temperature_unit(TemperatureUnit::Celsius);
    Settings { temperature_unit: TemperatureUnit::Fahrenheit }.apply();
    assert_eq!(Value::Celsius(0.0).display(), "32 °F");
    set_temperature_unit(TemperatureUnit::Celsius);
}
