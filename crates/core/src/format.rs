/// Decimal (SI) capacity as drive vendors print it: 2000409264 sectors -> "1.0 TB".
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[i]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_si_units() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1_024_209_543_168), "1.0 TB");
        assert_eq!(format_bytes(64_000_000_000), "64.0 GB");
    }
}
