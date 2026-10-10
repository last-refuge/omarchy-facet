use std::path::Path;
use std::time::Instant;

use facet_core::{Availability, Category, Inventory, Report};

const USAGE: &str = "usage: facet [--json] [--include-identifiers] [--status] [--celsius|--fahrenheit] [--diagnose] [--version] [--help]

Prints a hardware inventory for this machine.

  --json                 versioned JSON report (schema in docs/report-schema.md)
  --include-identifiers  include serial numbers, UUIDs and MAC addresses (hidden by default)
  --celsius, --fahrenheit  temperature unit for this run (default: your saved setting, else Fahrenheit;
                         change it in Facet's Preferences). JSON numbers are always Celsius.
  --status               a few live readings as JSON, fast enough to poll (used by the Omarchy bar widget)
  --diagnose             what Facet can and cannot read here, and how long each collector takes
  --version              print the version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    if args.iter().any(|a| a == "--version") {
        println!("facet {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Some(bad) = args.iter().find(|a| !matches!(a.as_str(), "--json" | "--include-identifiers" | "--diagnose" | "--status" | "--celsius" | "--fahrenheit")) {
        eprintln!("facet: unknown option {bad:?}\n{USAGE}");
        std::process::exit(2);
    }
    // Temperatures display in the saved unit (default Fahrenheit); a flag overrides it for this run.
    let mut settings = facet_core::Settings::load();
    if args.iter().any(|a| a == "--celsius") {
        settings.temperature_unit = facet_core::TemperatureUnit::Celsius;
    }
    if args.iter().any(|a| a == "--fahrenheit") {
        settings.temperature_unit = facet_core::TemperatureUnit::Fahrenheit;
    }
    settings.apply();
    if args.iter().any(|a| a == "--diagnose") {
        diagnose();
        return;
    }
    if args.iter().any(|a| a == "--status") {
        status();
        return;
    }
    let json = args.iter().any(|a| a == "--json");
    let include_ids = args.iter().any(|a| a == "--include-identifiers");

    let report = Report::new(facet_collectors::collect_all(), include_ids);
    if json {
        println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
    } else {
        print!("{}", report.to_text());
    }
}

/// Headline readings only, from the cheap collectors: safe to poll every few seconds.
fn status() {
    const NEEDED: [&str; 6] = ["system", "processor", "memory", "graphics", "power", "sensors"];
    let mut inv = Inventory::default();
    for (name, collect) in facet_collectors::COLLECTORS {
        if NEEDED.contains(name)
            && let Ok(c) = std::panic::catch_unwind(collect)
        {
            inv.components.extend(c);
        }
    }
    let machine = inv.in_category(Category::System).next().map(|c| c.title.clone()).unwrap_or_default();
    let out = serde_json::json!({
        "schema_version": 1,
        "generator": format!("facet {}", env!("CARGO_PKG_VERSION")),
        "machine": machine,
        "readings": facet_core::headline_readings(&inv),
    });
    println!("{}", serde_json::to_string(&out).expect("status serializes"));
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned())
}

fn readable(path: &str) -> &'static str {
    match std::fs::File::open(path) {
        Ok(_) => "readable",
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "permission denied (needs administrator access)",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "not present",
        Err(_) => "error",
    }
}

/// Support output: nothing in it identifies the machine or its owner (no serials, MACs, hostname, user).
fn diagnose() {
    println!("facet {}", env!("CARGO_PKG_VERSION"));
    println!("kernel        {}", read("/proc/sys/kernel/osrelease").unwrap_or_else(|| "unknown".into()));
    println!("session       {}", std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into()));
    println!("temperature   {}", facet_core::temperature_unit().label());
    println!("wayland       {}", if std::env::var_os("WAYLAND_DISPLAY").is_some() { "display set" } else { "no display set" });
    println!("x11           {}", if std::env::var_os("DISPLAY").is_some() { "display set" } else { "no display set" });

    println!("\nData sources");
    for (label, path) in [
        ("DMI identity (model, BIOS)", "/sys/class/dmi/id/product_name"),
        ("DMI serial numbers", "/sys/class/dmi/id/product_serial"),
        ("SMBIOS memory modules", "/sys/firmware/dmi/tables/DMI"),
        ("hwmon sensors", "/sys/class/hwmon"),
        ("power supplies", "/sys/class/power_supply"),
        ("PCI devices", "/sys/bus/pci/devices"),
        ("USB devices", "/sys/bus/usb/devices"),
        ("DRM displays (EDID)", "/sys/class/drm"),
        ("PCI id database", "/usr/share/hwdata/pci.ids"),
        ("USB id database", "/usr/share/hwdata/usb.ids"),
    ] {
        println!("  {label:<28} {}", readable(path));
    }
    let hotplug = {
        let h = std::thread::spawn(|| facet_collectors::hotplug::watch(|_| {}));
        std::thread::sleep(std::time::Duration::from_millis(200));
        if h.is_finished() { format!("unavailable ({})", h.join().ok().and_then(Result::err).map(|e| e.to_string()).unwrap_or_default()) } else { "available".to_owned() }
    };
    println!("  {:<28} {hotplug}", "hotplug (kernel uevents)");

    println!("\nGPU power states (Facet never wakes a sleeping GPU)");
    let mut any_gpu = false;
    if let Ok(rd) = std::fs::read_dir("/sys/bus/pci/devices") {
        let mut devs: Vec<_> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        devs.sort();
        for d in devs {
            let class = read(&d.join("class").to_string_lossy()).unwrap_or_default();
            if !class.starts_with("0x03") {
                continue;
            }
            any_gpu = true;
            let status = read(&d.join("power/runtime_status").to_string_lossy()).unwrap_or_else(|| "not reported".into());
            let vendor = read(&d.join("vendor").to_string_lossy()).unwrap_or_default();
            let boot = read(&d.join("boot_vga").to_string_lossy()).as_deref() == Some("1");
            println!("  {} vendor {vendor}{} runtime {status}", d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), if boot { " (boot GPU)" } else { "" });
        }
    }
    if !any_gpu {
        println!("  none found");
    }

    println!("\nCollectors");
    let total = Instant::now();
    let mut inv = Inventory::default();
    for (name, collect) in facet_collectors::COLLECTORS {
        let t = Instant::now();
        let comps = std::panic::catch_unwind(collect);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        match comps {
            Ok(c) => {
                println!("  {name:<12} {:>3} components  {ms:6.1} ms", c.len());
                inv.components.extend(c);
            }
            Err(_) => println!("  {name:<12} PANICKED (this is a bug; please report it)  {ms:6.1} ms"),
        }
    }
    println!("  total {:.1} ms", total.elapsed().as_secs_f64() * 1000.0);

    println!("\nField availability");
    let (mut avail, mut notrep, mut perm, mut failed, mut other) = (0, 0, 0, 0, 0);
    for c in &inv.components {
        for s in c.all_specs() {
            match s.value.availability {
                Availability::Available { .. } => avail += 1,
                Availability::NotReported => notrep += 1,
                Availability::PermissionRequired => perm += 1,
                Availability::Failed { .. } => failed += 1,
                _ => other += 1,
            }
        }
    }
    println!("  available {avail}, not reported {notrep}, permission required {perm}, failed {failed}, other {other}");
    for cat in Category::ALL {
        println!("  {:<16} {}", cat.label(), inv.in_category(cat).count());
    }
    let _ = Path::new("/");
}
