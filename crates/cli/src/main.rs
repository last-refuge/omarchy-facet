use facet_core::{Availability, Field, Report};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    let include_ids = args.iter().any(|a| a == "--include-identifiers");

    let mut report = Report::new(facet_collectors::storage::collect());
    if !include_ids {
        report.redact_identifiers();
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
        return;
    }

    for d in &report.storage {
        println!("{}", d.id);
        row("model", &d.model, |v| v.clone());
        row("firmware", &d.firmware, |v| v.clone());
        row("serial", &d.serial, |v| v.clone());
        row("capacity", &d.capacity_bytes, |b| facet_core::format_bytes(*b));
        row("transport", &d.transport, |t| format!("{t:?}"));
        println!();
    }
}

fn row<T>(label: &str, f: &Field<T>, show: impl Fn(&T) -> String) {
    let text = match &f.availability {
        Availability::Available { value } => show(value),
        Availability::NotReported => "(not reported)".into(),
        Availability::PermissionRequired => "(permission required)".into(),
        Availability::Disconnected => "(disconnected)".into(),
        Availability::Redacted => "(redacted)".into(),
        Availability::Failed { reason } => format!("(failed: {reason})"),
    };
    println!("  {label:<10} {text}");
}
