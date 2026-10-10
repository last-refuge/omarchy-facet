use facet_core::Report;

const USAGE: &str = "usage: facet [--json] [--include-identifiers] [--help]\n\nPrints a hardware inventory. Serial numbers, UUIDs and MAC addresses are hidden unless --include-identifiers is given.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return;
    }
    if let Some(bad) = args.iter().find(|a| !matches!(a.as_str(), "--json" | "--include-identifiers")) {
        eprintln!("facet: unknown option {bad:?}\n{USAGE}");
        std::process::exit(2);
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
