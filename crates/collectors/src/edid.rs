//! EDID parser (base block, CTA-861 and DisplayID extensions, best effort). Pure functions over bytes:
//! never panics on malformed input, never reads out of bounds, never invents values.

#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub pixel_clock_khz: u32,
    pub h_active: u32,
    pub v_active: u32,
    pub h_total: u32,
    pub v_total: u32,
    pub interlaced: bool,
    /// Where it came from: "base block", "CTA-861 extension", "DisplayID extension".
    pub origin: &'static str,
}

impl Timing {
    /// Refresh rate in Hz computed from the pixel clock and totals (field rate for interlaced).
    pub fn refresh_hz(&self) -> Option<f64> {
        let total = f64::from(self.h_total) * f64::from(self.v_total);
        if total <= 0.0 || self.pixel_clock_khz == 0 {
            return None;
        }
        let hz = f64::from(self.pixel_clock_khz) * 1000.0 / total;
        Some(if self.interlaced { hz * 2.0 } else { hz })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Edid {
    pub version: u8,
    pub revision: u8,
    /// Three-letter PNP manufacturer code, e.g. "DEL".
    pub pnp: String,
    pub product_code: u16,
    /// Numeric serial from the base block; 0 means "not set" and is reported as `None`.
    pub serial_number: Option<u32>,
    pub model_name: Option<String>,
    pub serial_text: Option<String>,
    pub week: Option<u8>,
    pub year: Option<u16>,
    /// Header byte 0xFF in the week field means `year` is the model year, not the build year.
    pub model_year: bool,
    pub digital: bool,
    pub bit_depth: Option<u8>,
    pub interface: Option<&'static str>,
    pub width_cm: Option<u8>,
    pub height_cm: Option<u8>,
    pub color_formats: Vec<&'static str>,
    pub preferred: Option<Timing>,
    pub extension_blocks: usize,
    pub base_checksum_ok: bool,
    /// False if any extension block we could read failed its checksum.
    pub extension_checksums_ok: bool,
    pub hdr_eotfs: Vec<&'static str>,
    pub hdr_max_luminance_nits: Option<f64>,
    pub colorimetry: Vec<&'static str>,
    /// Human-readable parse caveats (truncation, bad checksum, ...), surfaced to the user.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdidError {
    TooShort(usize),
    BadHeader,
}

impl std::fmt::Display for EdidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdidError::TooShort(n) => write!(f, "EDID is {n} bytes; a base block needs 128"),
            EdidError::BadHeader => write!(f, "EDID header signature is invalid"),
        }
    }
}

const HEADER: [u8; 8] = [0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0];

pub fn checksum_ok(block: &[u8]) -> bool {
    block.len() == 128 && block.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

pub fn parse(b: &[u8]) -> Result<Edid, EdidError> {
    if b.len() < 128 {
        return Err(EdidError::TooShort(b.len()));
    }
    if b[..8] != HEADER {
        return Err(EdidError::BadHeader);
    }
    let mut e = Edid { version: b[18], revision: b[19], ..Default::default() };

    let m = u16::from_be_bytes([b[8], b[9]]);
    let letters = [(m >> 10) & 0x1f, (m >> 5) & 0x1f, m & 0x1f];
    if letters.iter().all(|l| (1..=26).contains(l)) {
        e.pnp = letters.iter().map(|l| char::from(b'A' + (*l as u8) - 1)).collect();
    } else {
        e.pnp = "???".into();
        e.warnings.push("manufacturer code is not three letters A-Z".into());
    }
    e.product_code = u16::from_le_bytes([b[10], b[11]]);
    let serial = u32::from_le_bytes([b[12], b[13], b[14], b[15]]);
    e.serial_number = if serial == 0 { None } else { Some(serial) };
    match (b[16], b[17]) {
        (0xff, y) => {
            e.model_year = true;
            e.year = Some(1990 + u16::from(y));
        }
        (w, y) => {
            e.week = if (1..=54).contains(&w) { Some(w) } else { None };
            // Year 0 would be 1990, which is the "unset" convention in practice.
            e.year = if y == 0 { None } else { Some(1990 + u16::from(y)) };
        }
    }

    // Video input definition (byte 20).
    let vin = b[20];
    e.digital = vin & 0x80 != 0;
    if e.digital {
        if e.version == 1 && e.revision >= 4 {
            e.bit_depth = match (vin >> 4) & 7 {
                1 => Some(6),
                2 => Some(8),
                3 => Some(10),
                4 => Some(12),
                5 => Some(14),
                6 => Some(16),
                _ => None,
            };
            e.interface = match vin & 0x0f {
                1 => Some("DVI"),
                2 => Some("HDMI-a"),
                3 => Some("HDMI-b"),
                4 => Some("MDDI"),
                5 => Some("DisplayPort"),
                _ => None,
            };
        } else if vin & 1 != 0 {
            e.interface = Some("DVI");
        }
    }
    // Physical size: both nonzero = centimetres; otherwise byte 21/22 hold an aspect ratio or nothing.
    if b[21] != 0 && b[22] != 0 {
        e.width_cm = Some(b[21]);
        e.height_cm = Some(b[22]);
    }
    // Supported colour formats (byte 24 bits 4:3, digital only; analog uses a different meaning).
    if e.digital && e.version == 1 && e.revision >= 4 {
        e.color_formats.push("RGB 4:4:4");
        let f = (b[24] >> 3) & 3;
        if f & 1 != 0 {
            e.color_formats.push("YCbCr 4:4:4");
        }
        if f & 2 != 0 {
            e.color_formats.push("YCbCr 4:2:2");
        }
    }

    // Four 18-byte descriptors.
    for i in 0..4 {
        let d = &b[54 + 18 * i..72 + 18 * i];
        if d[0] != 0 || d[1] != 0 {
            if i == 0 {
                e.preferred = detailed_timing(d, "base block");
            }
            continue;
        }
        match d[3] {
            0xfc => e.model_name = descriptor_text(d),
            0xff => e.serial_text = descriptor_text(d),
            _ => {}
        }
    }

    e.extension_blocks = usize::from(b[126]);
    e.base_checksum_ok = checksum_ok(&b[..128]);
    if !e.base_checksum_ok {
        e.warnings.push("base block checksum mismatch; values may be corrupt".into());
    }
    e.extension_checksums_ok = true;
    let have = (b.len() / 128).saturating_sub(1);
    if have < e.extension_blocks {
        e.warnings.push(format!("EDID truncated: {} extension block(s) declared, {have} present", e.extension_blocks));
    }
    let mut first_cta_timing = None;
    let mut did_timing: Option<Timing> = None;
    for n in 1..=e.extension_blocks.min(have) {
        let blk = &b[128 * n..128 * (n + 1)];
        if !checksum_ok(blk) {
            e.extension_checksums_ok = false;
            e.warnings.push(format!("extension block {n} checksum mismatch"));
        }
        match blk[0] {
            0x02 => parse_cta(blk, &mut e, &mut first_cta_timing),
            0x70 => parse_displayid(blk, &mut did_timing),
            _ => {}
        }
    }
    if e.preferred.is_none() {
        e.preferred = did_timing.or(first_cta_timing);
    }
    Ok(e)
}

fn descriptor_text(d: &[u8]) -> Option<String> {
    let raw = &d[5..18];
    let end = raw.iter().position(|c| *c == 0x0a).unwrap_or(raw.len());
    let s: String = raw[..end].iter().map(|c| if (0x20..0x7f).contains(c) { char::from(*c) } else { '?' }).collect();
    let s = s.trim().to_owned();
    if s.is_empty() { None } else { Some(s) }
}

fn detailed_timing(d: &[u8], origin: &'static str) -> Option<Timing> {
    if d.len() < 18 {
        return None;
    }
    let clock = u32::from(u16::from_le_bytes([d[0], d[1]])) * 10;
    let h_active = u32::from(d[2]) | (u32::from(d[4] & 0xf0) << 4);
    let h_blank = u32::from(d[3]) | (u32::from(d[4] & 0x0f) << 8);
    let v_active = u32::from(d[5]) | (u32::from(d[7] & 0xf0) << 4);
    let v_blank = u32::from(d[6]) | (u32::from(d[7] & 0x0f) << 8);
    if clock == 0 || h_active == 0 || v_active == 0 {
        return None;
    }
    Some(Timing {
        pixel_clock_khz: clock,
        h_active,
        v_active,
        h_total: h_active + h_blank,
        v_total: v_active + v_blank,
        interlaced: d[17] & 0x80 != 0,
        origin,
    })
}

fn parse_cta(blk: &[u8], e: &mut Edid, first_timing: &mut Option<Timing>) {
    let d = usize::from(blk[2]);
    let flags = blk[3];
    if blk[1] >= 2 {
        for (bit, name) in [(0x20u8, "YCbCr 4:4:4"), (0x10, "YCbCr 4:2:2")] {
            if flags & bit != 0 && !e.color_formats.contains(&name) {
                if e.color_formats.is_empty() {
                    e.color_formats.push("RGB 4:4:4");
                }
                e.color_formats.push(name);
            }
        }
    }
    if !(4..=127).contains(&d) {
        return;
    }
    let mut i = 4;
    while i < d {
        let tag = blk[i] >> 5;
        let len = usize::from(blk[i] & 0x1f);
        if i + 1 + len > d {
            break;
        }
        let data = &blk[i + 1..i + 1 + len];
        if tag == 7 && !data.is_empty() {
            match data[0] {
                5 if data.len() >= 2 => {
                    const NAMES: [&str; 8] = ["xvYCC601", "xvYCC709", "sYCC601", "opYCC601", "opRGB", "BT.2020 cYCC", "BT.2020 YCC", "BT.2020 RGB"];
                    for (bit, n) in NAMES.iter().enumerate() {
                        if data[1] & (1 << bit) != 0 {
                            e.colorimetry.push(n);
                        }
                    }
                }
                6 if data.len() >= 3 => {
                    const EOTF: [&str; 4] = ["SDR gamma", "HDR gamma", "SMPTE ST 2084 (PQ)", "HLG"];
                    for (bit, n) in EOTF.iter().enumerate() {
                        if data[1] & (1 << bit) != 0 {
                            e.hdr_eotfs.push(n);
                        }
                    }
                    // data[2] = static metadata descriptors; data[3] = desired max luminance code.
                    if let Some(code) = data.get(3).filter(|c| **c != 0) {
                        e.hdr_max_luminance_nits = Some(50.0 * 2f64.powf(f64::from(*code) / 32.0));
                    }
                }
                _ => {}
            }
        }
        i += 1 + len;
    }
    let mut p = d;
    while p + 18 <= 127 {
        match detailed_timing(&blk[p..p + 18], "CTA-861 extension") {
            Some(t) => {
                if first_timing.is_none() {
                    *first_timing = Some(t);
                }
            }
            None => break,
        }
        p += 18;
    }
}

/// DisplayID 1.x (Type I, tag 0x03) and 2.0 (Type VII, tag 0x22) detailed timings; prefers the
/// timing flagged as preferred, else the first one.
fn parse_displayid(blk: &[u8], out: &mut Option<Timing>) {
    let v2 = blk[1] >= 0x20;
    let section = usize::from(blk[2]).min(121);
    let end = 5 + section;
    let (want_tag, clock_unit) = if v2 { (0x22u8, 1u32) } else { (0x03u8, 10u32) };
    let mut i = 5;
    while i + 3 <= end.min(127) {
        let tag = blk[i];
        let len = usize::from(blk[i + 2]);
        let start = i + 3;
        if start + len > 127 {
            break;
        }
        if tag == want_tag {
            for t in blk[start..start + len].as_chunks::<20>().0 {
                let clock = (u32::from(t[0]) | u32::from(t[1]) << 8 | u32::from(t[2]) << 16) + 1;
                let w = |lo: usize| u32::from(u16::from_le_bytes([t[lo], t[lo + 1]]) & 0x7fff) + 1;
                let (h_active, h_blank, v_active, v_blank) = (w(4), w(6), w(12), w(14));
                let timing = Timing {
                    pixel_clock_khz: clock * clock_unit,
                    h_active,
                    v_active,
                    h_total: h_active + h_blank,
                    v_total: v_active + v_blank,
                    interlaced: t[3] & 0x10 != 0,
                    origin: "DisplayID extension",
                };
                let preferred = t[3] & 0x80 != 0;
                if preferred || out.is_none() {
                    let was_preferred = preferred;
                    *out = Some(timing);
                    if was_preferred {
                        return;
                    }
                }
            }
        }
        i = start + len;
    }
}

/// Common vendors only; the raw three-letter code is always shown alongside. Unknown -> `None`.
pub fn vendor_name(pnp: &str) -> Option<&'static str> {
    Some(match pnp {
        "ACR" => "Acer",
        "AOC" => "AOC",
        "APP" => "Apple",
        "AUO" => "AU Optronics",
        "AUS" | "ASU" => "ASUS",
        "BNQ" => "BenQ",
        "BOE" => "BOE",
        "CMN" | "CMO" => "Chimei Innolux",
        "DEL" => "Dell",
        "EIZ" => "Eizo",
        "GBT" => "Gigabyte",
        "GSM" => "LG Electronics",
        "HSD" => "HannStar",
        "HKC" => "HKC",
        "HWP" => "HP",
        "IVM" => "Iiyama",
        "IVO" => "InfoVision",
        "LEN" => "Lenovo",
        "LGD" => "LG Display",
        "MEI" => "Panasonic",
        "MSI" => "MSI",
        "NEC" => "NEC",
        "PHL" => "Philips",
        "SAM" => "Samsung",
        "SDC" => "Samsung Display",
        "SHP" => "Sharp",
        "SNY" => "Sony",
        "TSB" => "Toshiba",
        "VSC" => "ViewSonic",
        _ => return None,
    })
}

pub fn diagonal_inches(width_cm: u8, height_cm: u8) -> f64 {
    let (w, h) = (f64::from(width_cm), f64::from(height_cm));
    (w * w + h * h).sqrt() / 2.54
}
