//! Batteries, AC adapters and USB-C power sources from `/sys/class/power_supply`. Read-only.
//! Derived values (Wh from charge x voltage, health, time to empty) are labelled as derived in
//! their source string; nothing is computed from partial data. Machines without any power supply
//! (desktops) yield no components.

use std::path::Path;

use facet_core::{Availability, Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{list_dir, parse_field, read_field, source, text_spec};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    list_dir(root, "sys/class/power_supply").into_iter().filter_map(|n| read_supply(root, &n)).collect()
}

fn num(root: &Path, rel: &str) -> Field<i64> {
    parse_field(read_field(root, rel), |s| s.parse::<i64>().map_err(|e| format!("not a number: {e}")))
}

/// A numeric attribute scaled by `div` into a `Value`; `unknown_zero` treats 0 as "driver does not report".
fn scaled(root: &Path, rel: &str, div: f64, unknown_zero: bool, make: impl FnOnce(f64) -> Value) -> Field<Value> {
    let f = num(root, rel);
    if unknown_zero && f.value() == Some(&0) {
        return Field::not_reported(format!("{} (reads 0: not reported by the driver)", f.source));
    }
    parse_field(f, |v| Ok(make(*v as f64 / div)))
}

fn wh(x: f64) -> Value {
    Value::Text(format!("{x:.2} Wh"))
}

fn ah(x: f64) -> Value {
    Value::Text(format!("{x:.3} Ah"))
}

fn amps(x: f64) -> Value {
    Value::Text(format!("{x:.3} A"))
}

fn duration(hours: f64) -> String {
    let mins = (hours * 60.0).round() as u64;
    if mins >= 60 { format!("{} h {:02} min", mins / 60, mins % 60) } else { format!("{mins} min") }
}

/// Energy-like quantity pair: either `energy_*` (µWh) or `charge_*` (µAh).
struct Quantity {
    /// Value in µWh or µAh.
    micro: i64,
    /// true if µWh, false if µAh.
    is_energy: bool,
    src: String,
}

fn quantity(root: &Path, base: &str, what: &str) -> Option<Quantity> {
    for (prefix, is_energy) in [("energy", true), ("charge", false)] {
        let rel = format!("{base}/{prefix}_{what}");
        if let Some(&micro) = num(root, &rel).value() {
            return Some(Quantity { micro, is_energy, src: source(&rel) });
        }
    }
    None
}

/// Display value of a quantity in Wh (exact for energy_*, charge x design voltage otherwise) or Ah.
fn quantity_field(q: Option<&Quantity>, vmin_uv: Option<i64>, missing_src: &str) -> Field<Value> {
    match q {
        None => Field::not_reported(missing_src),
        Some(q) if q.micro < 0 => Field::failed(format!("negative value {}", q.micro), &q.src),
        Some(q) if q.is_energy => Field::available(wh(q.micro as f64 / 1e6), &q.src),
        Some(q) => match vmin_uv {
            Some(v) if v > 0 => Field::available(wh(q.micro as f64 * v as f64 / 1e12), format!("{} x voltage_min_design (derived)", q.src)),
            _ => Field::available(ah(q.micro as f64 / 1e6), &q.src),
        },
    }
}

fn read_supply(root: &Path, name: &str) -> Option<Component> {
    let base = format!("sys/class/power_supply/{name}");
    let ty = read_field(root, &format!("{base}/type")).value().cloned()?;
    let id = format!("power:{name}");
    let t = |attr: &str| read_field(root, &format!("{base}/{attr}"));

    if ty.eq_ignore_ascii_case("battery") {
        return Some(battery(root, name, &base, &id));
    }

    // Non-battery source: mains adapter, USB / USB-C, wireless pad, UPS.
    let online = parse_field(t("online"), |s| match s.as_str() {
        "0" => Ok(Value::Bool(false)),
        "1" | "2" => Ok(Value::Bool(true)),
        o => Err(format!("unexpected value {o:?}")),
    });
    let is_ucsi = name.starts_with("ucsi-");
    let title = match ty.as_str() {
        "Mains" => "AC adapter".to_owned(),
        "USB" | "USB_C" | "USB_PD" | "USB_PD_DRP" | "USB_DCP" | "USB_CDP" | "USB_ACA" if is_ucsi => {
            format!("USB-C power source ({})", name.rsplit(':').next().unwrap_or(name))
        }
        "USB" | "USB_C" | "USB_PD" | "USB_PD_DRP" | "USB_DCP" | "USB_CDP" | "USB_ACA" => format!("USB power source ({name})"),
        other => format!("{other} power source ({name})"),
    };
    let sub = match online.value() {
        Some(Value::Bool(true)) => "Online",
        Some(Value::Bool(false)) => "Offline",
        _ => "Online state not reported",
    };
    let zero_unknown = |attr: &str, div: f64, make: fn(f64) -> Value| scaled(root, &format!("{base}/{attr}"), div, true, make);
    let mut specs = vec![
        Spec::new("Online", online.clone()).live(),
        text_spec("Type", t("type")),
        text_spec("USB type", t("usb_type")),
        text_spec("Status", t("status")),
        text_spec("Scope", t("scope")),
        Spec::new("Maximum current", zero_unknown("current_max", 1e6, amps)).rated(),
        Spec::new("Maximum voltage", zero_unknown("voltage_max", 1e6, Value::Volts)).rated(),
        Spec::new("Minimum voltage", zero_unknown("voltage_min", 1e6, Value::Volts)).rated(),
        Spec::new("Present voltage", zero_unknown("voltage_now", 1e6, Value::Volts)).live(),
        Spec::new("Present current", scaled(root, &format!("{base}/current_now"), 1e6, false, amps)).live(),
    ];
    specs.retain(|s| !matches!(s.value.availability, Availability::NotReported) || s.label == "Online" || s.label == "Maximum current");
    Some(
        Component::new(&id, Category::Power, &title)
            .subtitle(sub)
            .summary(vec![Spec::new("Online", online).live(), text_spec("Type", t("type"))])
            .group(SpecGroup::new("Power source", specs))
            .group(SpecGroup::new("Device", vec![Spec::text("Kernel name", name, &source(&base))]).technical()),
    )
}

fn battery(root: &Path, name: &str, base: &str, id: &str) -> Component {
    let t = |attr: &str| read_field(root, &format!("{base}/{attr}"));
    let src = |attr: &str| source(&format!("{base}/{attr}"));

    let present = t("present");
    let not_present = present.value().map(String::as_str) == Some("0");
    let model = t("model_name");
    let scope = t("scope");
    let peripheral = scope.value().map(String::as_str) == Some("Device");
    let title = match model.value() {
        Some(m) if peripheral => format!("{m} (peripheral battery)"),
        Some(m) => format!("Battery {m}"),
        None if peripheral => format!("Peripheral battery {name}"),
        None => format!("Battery {name}"),
    };

    let status = t("status");
    let capacity = parse_field(t("capacity"), |s| match s.parse::<f64>() {
        Ok(p) if (0.0..=100.0).contains(&p) => Ok(Value::Percent(p)),
        Ok(p) => Err(format!("capacity {p} outside 0-100")),
        Err(e) => Err(e.to_string()),
    });

    let vmin = num(root, &format!("{base}/voltage_min_design")).value().copied();
    let design = quantity(root, base, "full_design");
    let full = quantity(root, base, "full");
    let now = quantity(root, base, "now");
    let missing = |attr: &str| src(attr);

    let design_f = quantity_field(design.as_ref(), vmin, &missing("energy_full_design"));
    let full_f = quantity_field(full.as_ref(), vmin, &missing("energy_full"));
    let now_f = quantity_field(now.as_ref(), vmin, &missing("energy_now"));

    // Health: only when both are present, positive, and measured in the same unit.
    let health = match (&design, &full) {
        (Some(d), Some(f)) if d.is_energy == f.is_energy && d.micro > 0 && f.micro >= 0 => {
            let kind = if d.is_energy { "energy" } else { "charge" };
            Field::available(
                Value::Percent(f.micro as f64 * 100.0 / d.micro as f64),
                format!("derived: {kind}_full / {kind}_full_design ({}, {})", f.src, d.src),
            )
        }
        (Some(d), _) if d.micro <= 0 => Field::failed(format!("design capacity is {}", d.micro), d.src.clone()),
        _ => Field::not_reported("derived: needs both full and full_design capacity (not exposed)"),
    };

    let cycles = match num(root, &format!("{base}/cycle_count")) {
        f if f.value().is_some_and(|c| *c < 0) => Field::not_reported(format!("{} (reported {}: unknown)", f.source, f.value().unwrap())),
        f => parse_field(f, |c| Ok(Value::Count(*c as u64))),
    };

    // Electrical, live. Sign follows the kernel convention only loosely across drivers, so show magnitudes;
    // the status field gives the direction.
    let voltage = scaled(root, &format!("{base}/voltage_now"), 1e6, false, Value::Volts);
    let power_attr = num(root, &format!("{base}/power_now"));
    let current_attr = num(root, &format!("{base}/current_now"));
    let (power_w, power_src_note) = match (power_attr.value(), current_attr.value(), num(root, &format!("{base}/voltage_now")).value()) {
        (Some(p), _, _) => (Some(p.unsigned_abs() as f64 / 1e6), src("power_now")),
        (None, Some(c), Some(v)) => (Some(c.unsigned_abs() as f64 * v.unsigned_abs() as f64 / 1e12), format!("{} x {} (derived)", src("current_now"), src("voltage_now"))),
        _ => (None, src("power_now")),
    };
    let power = match power_w {
        Some(w) => Field::available(Value::Watts(w), power_src_note),
        None => Field::not_reported(power_src_note),
    };
    let current = parse_field(current_attr.clone(), |c| Ok(amps(c.unsigned_abs() as f64 / 1e6)));

    // Time to empty: only while discharging, from remaining capacity and instantaneous draw.
    let discharging = status.value().map(String::as_str) == Some("Discharging");
    let time_to_empty = if !discharging {
        Field::not_reported("estimate: only available while discharging")
    } else {
        match (&now, power_attr.value(), current_attr.value()) {
            (Some(n), Some(p), _) if n.is_energy && *p != 0 && n.micro >= 0 => {
                Field::available(Value::Text(format!("about {}", duration(n.micro as f64 / p.unsigned_abs() as f64))), format!("estimate: {} / {}", n.src, src("power_now")))
            }
            (Some(n), _, Some(c)) if !n.is_energy && *c != 0 && n.micro >= 0 => {
                Field::available(Value::Text(format!("about {}", duration(n.micro as f64 / c.unsigned_abs() as f64))), format!("estimate: {} / {}", n.src, src("current_now")))
            }
            _ => Field::not_reported("estimate: needs remaining capacity and a non-zero draw"),
        }
    };

    let pct = |attr: &str| parse_field(t(attr), |s| s.parse::<f64>().map_err(|e| e.to_string()).and_then(|p| if (0.0..=100.0).contains(&p) { Ok(Value::Percent(p)) } else { Err(format!("{p} outside 0-100")) }));
    let mut limits = Vec::new();
    for (attr, label) in [("charge_control_start_threshold", "Start charging below (read-only)"), ("charge_control_end_threshold", "Stop charging at (read-only)")] {
        if !matches!(t(attr).availability, Availability::NotReported) {
            limits.push(Spec::new(label, pct(attr)));
        }
    }

    let sub = [status.value().cloned(), capacity.value().map(Value::display)].into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let mut c = Component::new(id, Category::Power, &title);
    c = c.subtitle(&if not_present { "Not present".to_owned() } else { sub });
    c = c.summary(vec![
        Spec::new("Charge", capacity).live(),
        text_spec("Status", status).live(),
        Spec::new("Battery health", health.clone()),
    ]);
    c = c.group(SpecGroup::new(
        "Capacity",
        vec![
            Spec::new("Design capacity", design_f).rated(),
            Spec::new("Full-charge capacity", full_f),
            Spec::new("Remaining", now_f).live(),
            Spec::new("Battery health", health),
            Spec::new("Charge cycles", cycles),
        ],
    ));
    c = c.group(SpecGroup::new(
        "Live",
        vec![
            Spec::new("Voltage", voltage).live(),
            Spec::new("Current", current).live(),
            Spec::new("Power draw", power).live(),
            Spec::new("Time to empty (estimate)", time_to_empty).live(),
            text_spec("Charge level", t("capacity_level")).live(),
            text_spec("Charge type", t("charge_types")).live(),
        ],
    ));
    if !limits.is_empty() {
        c = c.group(SpecGroup::new("Charge limits", limits));
    }
    c.group(SpecGroup::new(
        "Identity",
        vec![
            text_spec("Model", model),
            text_spec("Manufacturer", t("manufacturer")),
            text_spec("Technology", t("technology")),
            text_spec("Serial number", t("serial_number")).identifier(),
            Spec::new("Voltage (design minimum)", scaled(root, &format!("{base}/voltage_min_design"), 1e6, true, Value::Volts)).rated(),
            Spec::text("Kernel name", name, &source(base)),
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    fn bat(files: &[(&str, &str)]) -> Fx {
        let mut v: Vec<(String, String)> = vec![("sys/class/power_supply/BAT0/type".into(), "Battery\n".into())];
        v.extend(files.iter().map(|(k, b)| (format!("sys/class/power_supply/BAT0/{k}"), (*b).to_owned())));
        let refs: Vec<(&str, &str)> = v.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        Fx::new(&refs)
    }

    #[test]
    fn full_energy_battery_matches_reference_values() {
        let fx = bat(&[
            ("status", "Discharging\n"),
            ("capacity", "50\n"),
            ("technology", "Li-poly\n"),
            ("manufacturer", "SMP\n"),
            ("model_name", "L24M4PC0\n"),
            ("serial_number", " 3607\n"),
            ("cycle_count", "12\n"),
            ("energy_full_design", "80000000\n"),
            ("energy_full", "72000000\n"),
            ("energy_now", "36000000\n"),
            ("voltage_min_design", "15520000\n"),
            ("voltage_now", "15745000\n"),
            ("power_now", "18000000\n"),
            ("charge_control_end_threshold", "80\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "power:BAT0");
        assert_eq!(c.category, Category::Power);
        assert_eq!(spec(c, "Design capacity").display(), "80.00 Wh");
        assert!(spec(c, "Design capacity").rated);
        assert_eq!(spec(c, "Battery health").value.value(), Some(&Value::Percent(90.0)));
        assert!(spec(c, "Battery health").value.source.contains("derived"));
        assert_eq!(spec(c, "Charge cycles").display(), "12");
        assert_eq!(spec(c, "Charge").display(), "50%");
        assert!(spec(c, "Charge").live);
        assert_eq!(spec(c, "Voltage").value.value(), Some(&Value::Volts(15.745)));
        assert_eq!(spec(c, "Power draw").value.value(), Some(&Value::Watts(18.0)));
        assert_eq!(spec(c, "Time to empty (estimate)").display(), "about 2 h 00 min");
        assert!(spec(c, "Time to empty (estimate)").value.source.starts_with("estimate"));
        assert_eq!(spec(c, "Stop charging at (read-only)").display(), "80%");
        assert!(spec(c, "Serial number").identifier);
        assert_eq!(spec(c, "Serial number").display(), "3607");
    }

    #[test]
    fn charge_only_battery_derives_wh_from_design_voltage() {
        let fx = bat(&[
            ("status", "Charging\n"),
            ("charge_full_design", "5000000\n"),
            ("charge_full", "4500000\n"),
            ("charge_now", "1000000\n"),
            ("voltage_min_design", "11400000\n"),
            ("current_now", "2000000\n"),
            ("voltage_now", "12000000\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Design capacity").display(), "57.00 Wh");
        assert!(spec(c, "Design capacity").value.source.contains("derived"));
        assert_eq!(spec(c, "Battery health").value.value(), Some(&Value::Percent(90.0)));
        assert_eq!(spec(c, "Power draw").value.value(), Some(&Value::Watts(24.0)));
        assert!(spec(c, "Power draw").value.source.contains("derived"));
        assert_eq!(spec(c, "Time to empty (estimate)").display(), "Not reported", "not discharging");
    }

    #[test]
    fn charge_without_voltage_shows_ah_and_health_still_ratio() {
        let fx = bat(&[("charge_full_design", "5000000\n"), ("charge_full", "4000000\n")]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Design capacity").display(), "5.000 Ah");
        assert_eq!(spec(c, "Battery health").value.value(), Some(&Value::Percent(80.0)));
    }

    #[test]
    fn unusual_values_are_reported_honestly() {
        let fx = bat(&[
            ("cycle_count", "-1\n"),
            ("energy_full_design", "50000000\n"),
            ("energy_full", "52500000\n"),
            ("capacity", "140\n"),
            ("serial_number", "\n"),
            ("voltage_now", "abc\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Charge cycles").display(), "Not reported");
        assert_eq!(spec(c, "Battery health").value.value(), Some(&Value::Percent(105.0)));
        assert!(matches!(spec(c, "Charge").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Serial number").display(), "Not reported");
        assert!(matches!(spec(c, "Voltage").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Power draw").display(), "Not reported");
    }

    #[test]
    fn health_requires_both_capacities() {
        let fx = bat(&[("energy_full", "50000000\n")]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Battery health").display(), "Not reported");
        let fx = bat(&[("energy_full_design", "0\n"), ("energy_full", "5\n")]);
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "Battery health").value.availability, Availability::Failed { .. }));
    }

    #[test]
    fn mains_and_usbc_sources() {
        let fx = Fx::new(&[
            ("sys/class/power_supply/ADP0/type", "Mains\n"),
            ("sys/class/power_supply/ADP0/online", "1\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/type", "USB\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/online", "1\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/usb_type", "C [PD] PD_PPS\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/current_max", "0\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/current_now", "5000000\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:001/voltage_now", "0\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:002/type", "USB\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:002/online", "0\n"),
            ("sys/class/power_supply/ucsi-source-psy-USBC000:002/current_max", "3000000\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["power:ADP0", "power:ucsi-source-psy-USBC000:001", "power:ucsi-source-psy-USBC000:002"]);
        assert_eq!(v[0].title, "AC adapter");
        assert_eq!(spec(&v[0], "Online").display(), "Yes");
        assert_eq!(v[1].title, "USB-C power source (001)");
        assert_eq!(spec(&v[1], "USB type").display(), "C [PD] PD_PPS");
        assert_eq!(spec(&v[1], "Maximum current").display(), "Not reported");
        assert_eq!(spec(&v[1], "Present current").display(), "5.000 A");
        assert_eq!(spec(&v[2], "Online").display(), "No");
        assert_eq!(spec(&v[2], "Maximum current").display(), "3.000 A");
        assert!(spec(&v[2], "Maximum current").rated);
    }

    #[test]
    fn desktop_without_supplies_or_with_junk_entries_is_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
        let fx = Fx::new(&[("sys/class/power_supply/odd/uevent", "x\n")]);
        assert!(collect_from(fx.path()).is_empty());
    }
}
