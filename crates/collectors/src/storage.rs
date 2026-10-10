//! Physical block devices from sysfs. Partitions and logical devices (dm, zram, loop, md) are excluded.

use std::fs;
use std::path::Path;

use facet_core::{Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{list_dir, parse_field, read_field, source, text_spec};


/// Live system: sysfs from `/` and drive health from UDisks2 over the system bus.
pub fn collect() -> Vec<Component> {
    collect_with(Path::new("/"), &health::ZbusUdisks::new(), health::now_unix())
}

/// Fixture/alternate root: sysfs is read from `root`; the system bus is NOT contacted (it describes the
/// live machine, not the fixture), so the Health group reports UDisks2 as not queried.
pub fn collect_from(root: &Path) -> Vec<Component> {
    if root == Path::new("/") {
        return collect();
    }
    collect_with(root, &health::Unqueried, health::now_unix())
}

/// Collector core with an injectable SMART source and clock.
pub fn collect_with(root: &Path, smart: &dyn health::SmartSource, now: u64) -> Vec<Component> {
    list_dir(root, "sys/block")
        .into_iter()
        .filter(|n| is_physical(n))
        .map(|n| {
            let c = read_device(root, &n);
            c.group(health::health_group(root, &n, smart, now))
        })
        .collect()
}

fn is_physical(name: &str) -> bool {
    !["dm-", "zram", "loop", "ram", "md", "sr", "fd", "nbd"].iter().any(|p| name.starts_with(p))
}

fn parse_flag(s: &str) -> Result<bool, String> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        other => Err(format!("unexpected flag value {other:?}")),
    }
}

/// Interface name from the device name and the (symlink) path of its sysfs entry.
pub fn transport(root: &Path, name: &str) -> Option<&'static str> {
    if name.starts_with("nvme") {
        return Some("NVMe");
    }
    if name.starts_with("mmcblk") {
        return Some("SD/eMMC");
    }
    if name.starts_with("vd") {
        return Some("VirtIO");
    }
    let link = fs::read_link(crate::util::at(root, &format!("sys/block/{name}"))).ok()?;
    let p = link.to_string_lossy();
    if p.contains("/usb") {
        Some("USB")
    } else if p.contains("/ata") {
        Some("SATA")
    } else if p.contains("/virtio") {
        Some("VirtIO")
    } else {
        None
    }
}

fn media_type(rotational: Option<bool>, transport: Option<&str>) -> Option<&'static str> {
    match (rotational, transport) {
        (_, Some("NVMe")) => Some("NVMe solid-state drive"),
        (Some(true), _) => Some("Hard disk drive"),
        (Some(false), Some("SD/eMMC")) => Some("Flash storage"),
        (Some(false), _) => Some("Solid-state drive"),
        _ => None,
    }
}

fn read_device(root: &Path, name: &str) -> Component {
    let dev = format!("sys/block/{name}/device");
    let model = read_field(root, &format!("{dev}/model"));
    let vendor = read_field(root, &format!("{dev}/vendor"));
    let serial = read_field(root, &format!("{dev}/serial"));
    let firmware = {
        let f = read_field(root, &format!("{dev}/firmware_rev"));
        if f.value().is_some() { f } else { read_field(root, &format!("{dev}/rev")) }
    };
    // sysfs `size` is always in 512-byte sectors regardless of the device's sector size.
    let capacity = parse_field(read_field(root, &format!("sys/block/{name}/size")), |s| {
        s.parse::<u64>().map(|n| Value::Bytes(n.saturating_mul(512))).map_err(|e| e.to_string())
    });
    let rotational = parse_field(read_field(root, &format!("sys/block/{name}/queue/rotational")), |s| parse_flag(s));
    let removable = parse_field(read_field(root, &format!("sys/block/{name}/removable")), |s| parse_flag(s));
    let iface = transport(root, name);
    let media = media_type(rotational.value().copied(), iface);

    let title = model.value().cloned().unwrap_or_else(|| format!("Storage device {name}"));
    let subtitle = [capacity.value().map(Value::display), media.map(str::to_owned)].into_iter().flatten().collect::<Vec<_>>().join(" · ");

    let bool_spec = |label: &str, f: Field<bool>| Spec::new(label, parse_field(f, |b| Ok(Value::Bool(*b))));
    let mut c = Component::new(&format!("storage:{name}"), Category::Storage, &title);
    if !subtitle.is_empty() {
        c = c.subtitle(&subtitle);
    }
    c.summary(vec![
        Spec::new("Capacity", capacity),
        match media {
            Some(m) => Spec::text("Type", m, &source(&format!("sys/block/{name}/queue/rotational"))),
            None => Spec::missing("Type", &source(&format!("sys/block/{name}/queue/rotational"))),
        },
        match iface {
            Some(i) => Spec::text("Interface", i, &source(&format!("sys/block/{name}"))),
            None => Spec::missing("Interface", &source(&format!("sys/block/{name}"))),
        },
    ])
    .group(SpecGroup::new("Identity", vec![
        text_spec("Model", model),
        text_spec("Vendor", vendor),
        text_spec("Firmware", firmware),
        text_spec("Serial number", serial).identifier(),
    ]))
    .group(SpecGroup::new("Device", vec![
        Spec::text("Kernel name", name, &source(&format!("sys/block/{name}"))),
        bool_spec("Removable", removable),
        bool_spec("Rotational", rotational),
    ]).technical())
}

/// Drive health: UDisks2 SMART properties (cached, read via D-Bus Properties only) plus live hwmon temperature.
///
/// INSPECTION ONLY. The D-Bus client calls exactly three things: `org.freedesktop.DBus.NameHasOwner` on the
/// bus daemon (so a stopped UDisks2 is never bus-activated by us) and `org.freedesktop.DBus.Properties`
/// `Get`/`GetAll` on UDisks2 objects. No UDisks2 method (SmartUpdate, SmartGetAttributes, self-tests, ...)
/// is ever invoked.
pub mod health {
    use std::cell::OnceCell;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use facet_core::{Field, Spec, SpecGroup, Value};

    use crate::sensors::milli_temp;
    use crate::util::{list_dir, read_field, source};

    const BUS: &str = "org.freedesktop.UDisks2";
    const NVME: &str = "org.freedesktop.UDisks2.NVMe.Controller";
    const ATA: &str = "org.freedesktop.UDisks2.Drive.Ata";
    const BLOCK: &str = "org.freedesktop.UDisks2.Block";

    pub fn now_unix() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    /// A D-Bus property value, reduced to what the health mapping needs.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Prop {
        Bool(bool),
        U64(u64),
        I64(i64),
        F64(f64),
        Str(String),
        StrList(Vec<String>),
    }

    impl Prop {
        fn as_f64(&self) -> Option<f64> {
            match self {
                Prop::U64(v) => Some(*v as f64),
                Prop::I64(v) => Some(*v as f64),
                Prop::F64(v) => Some(*v),
                _ => None,
            }
        }

        fn as_u64(&self) -> Option<u64> {
            match self {
                Prop::U64(v) => Some(*v),
                Prop::I64(v) => u64::try_from(*v).ok(),
                Prop::F64(v) if *v >= 0.0 && v.fract() == 0.0 => Some(*v as u64),
                _ => None,
            }
        }
    }

    pub type Props = HashMap<String, Prop>;

    /// The UDisks2 drive object behind a block device, with the property maps of the interfaces we read
    /// (`None` = the object does not implement that interface).
    #[derive(Debug, Clone, Default)]
    pub struct DriveInfo {
        pub path: String,
        pub nvme: Option<Props>,
        pub ata: Option<Props>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum UdisksError {
        /// No system bus, or UDisks2 is not running / not answering.
        Unavailable(String),
        /// UDisks2 knows no drive for this block device (e.g. virtual device).
        NoDrive,
        Failed(String),
    }

    /// Source of SMART properties; tests inject canned maps.
    pub trait SmartSource {
        fn drive_for(&self, block_device: &str) -> Result<DriveInfo, UdisksError>;
    }

    /// Used for fixture roots: the live bus is deliberately not consulted.
    pub struct Unqueried;

    impl SmartSource for Unqueried {
        fn drive_for(&self, _: &str) -> Result<DriveInfo, UdisksError> {
            Err(UdisksError::Unavailable("not queried for a non-live root".into()))
        }
    }

    /// Blocking zbus client, connected lazily, 2 s method timeout.
    pub struct ZbusUdisks {
        conn: OnceCell<Result<zbus::blocking::Connection, String>>,
    }

    impl Default for ZbusUdisks {
        fn default() -> Self {
            Self::new()
        }
    }

    fn flatten(v: &zbus::zvariant::Value<'_>) -> Option<Prop> {
        use zbus::zvariant::Value as V;
        Some(match v {
            V::Bool(b) => Prop::Bool(*b),
            V::U8(n) => Prop::U64(u64::from(*n)),
            V::U16(n) => Prop::U64(u64::from(*n)),
            V::U32(n) => Prop::U64(u64::from(*n)),
            V::U64(n) => Prop::U64(*n),
            V::I16(n) => Prop::I64(i64::from(*n)),
            V::I32(n) => Prop::I64(i64::from(*n)),
            V::I64(n) => Prop::I64(*n),
            V::F64(n) => Prop::F64(*n),
            V::Str(s) => Prop::Str(s.to_string()),
            V::ObjectPath(p) => Prop::Str(p.to_string()),
            V::Value(inner) => return flatten(inner),
            V::Array(a) => Prop::StrList(a.iter().filter_map(|x| if let V::Str(s) = x { Some(s.to_string()) } else { None }).collect()),
            _ => return None,
        })
    }

    impl ZbusUdisks {
        pub fn new() -> Self {
            Self { conn: OnceCell::new() }
        }

        fn connection(&self) -> Result<&zbus::blocking::Connection, UdisksError> {
            let r = self.conn.get_or_init(|| {
                let c = zbus::blocking::connection::Builder::system()
                    .and_then(|b| b.method_timeout(Duration::from_secs(2)).build())
                    .map_err(|e| format!("system bus: {e}"))?;
                // Ask the bus daemon whether UDisks2 is running, so we never bus-activate it.
                let owned: bool = c
                    .call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "NameHasOwner", &BUS)
                    .and_then(|m| m.body().deserialize())
                    .map_err(|e| format!("NameHasOwner: {e}"))?;
                if owned { Ok(c) } else { Err("org.freedesktop.UDisks2 is not running".into()) }
            });
            r.as_ref().map_err(|e| UdisksError::Unavailable(e.clone()))
        }

        fn get_all(&self, path: &str, iface: &str) -> Result<Option<Props>, UdisksError> {
            let c = self.connection()?;
            match c.call_method(Some(BUS), path, Some("org.freedesktop.DBus.Properties"), "GetAll", &iface) {
                Ok(m) => {
                    let map: HashMap<String, zbus::zvariant::OwnedValue> = m.body().deserialize().map_err(|e| UdisksError::Failed(e.to_string()))?;
                    Ok(Some(map.iter().filter_map(|(k, v)| flatten(v).map(|p| (k.clone(), p))).collect()))
                }
                // The object exists but does not implement this interface.
                Err(zbus::Error::MethodError(name, ..)) if name.as_str().ends_with("UnknownInterface") || name.as_str().ends_with("InvalidArgs") => Ok(None),
                Err(e) => Err(UdisksError::Failed(e.to_string())),
            }
        }
    }

    impl SmartSource for ZbusUdisks {
        fn drive_for(&self, block: &str) -> Result<DriveInfo, UdisksError> {
            let c = self.connection()?;
            let block_path = format!("/org/freedesktop/UDisks2/block_devices/{block}");
            let drive: zbus::zvariant::OwnedValue = c
                .call_method(Some(BUS), block_path.as_str(), Some("org.freedesktop.DBus.Properties"), "Get", &(BLOCK, "Drive"))
                .and_then(|m| m.body().deserialize())
                .map_err(|e| match &e {
                    zbus::Error::MethodError(n, ..) if n.as_str().ends_with("ServiceUnknown") || n.as_str().ends_with("NameHasNoOwner") => UdisksError::Unavailable(e.to_string()),
                    zbus::Error::MethodError(n, ..) if n.as_str().ends_with("UnknownObject") || n.as_str().ends_with("UnknownMethod") => UdisksError::NoDrive,
                    _ => UdisksError::Failed(e.to_string()),
                })?;
            let path = match flatten(&drive) {
                Some(Prop::Str(p)) if p != "/" => p,
                _ => return Err(UdisksError::NoDrive),
            };
            Ok(DriveInfo { nvme: self.get_all(&path, NVME)?, ata: self.get_all(&path, ATA)?, path })
        }
    }

    // ---- mapping -------------------------------------------------------------------------------------

    /// `YYYY-MM-DD HH:MM UTC` from a unix timestamp.
    pub fn format_utc(ts: u64) -> String {
        let days = (ts / 86_400) as i64;
        let secs = ts % 86_400;
        // Howard Hinnant's civil_from_days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, (secs % 3600) / 60)
    }

    fn ago(now: u64, ts: u64) -> String {
        match now.checked_sub(ts) {
            None => "in the future".into(),
            Some(s) if s < 90 => "just now".into(),
            Some(s) if s < 5400 => format!("{} min ago", s / 60),
            Some(s) if s < 172_800 => format!("{} h ago", s / 3600),
            Some(s) => format!("{} days ago", s / 86_400),
        }
    }

    /// Kelvin -> Celsius. 0 K means "unknown" in UDisks2 and is not a temperature.
    pub fn kelvin_to_celsius(k: f64) -> Option<f64> {
        (k > 0.0).then_some(k - 273.15)
    }

    /// `SmartCriticalWarning` list -> text. Empty means the drive reports none; it is not a claim of health.
    pub fn critical_warning_text(w: &[String]) -> String {
        if w.is_empty() { "None reported".into() } else { w.join(", ") }
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Fresh {
        /// SmartUpdated present and non-zero.
        At(u64),
        /// SmartUpdated == 0: UDisks2 never read SMART from this drive.
        Never,
        /// SmartUpdated missing from the property map.
        Unknown,
    }

    struct Iface<'a> {
        path: &'a str,
        name: &'static str,
        props: Option<&'a Props>,
    }

    impl Iface<'_> {
        fn src(&self, key: &str) -> String {
            format!("dbus:{BUS} {} {}.{key}", self.path, self.name)
        }

        fn fresh(&self) -> Fresh {
            match self.props.and_then(|p| p.get("SmartUpdated")).and_then(Prop::as_u64) {
                Some(0) => Fresh::Never,
                Some(t) => Fresh::At(t),
                None => Fresh::Unknown,
            }
        }

        /// One property as a field. `cached` values depend on `SmartUpdated`; `conv` returns `Ok(None)` when
        /// the value means "unknown".
        fn field(&self, key: &str, cached: bool, conv: impl FnOnce(&Prop) -> Result<Option<Value>, String>) -> Field<Value> {
            let Some(props) = self.props else {
                return Field::not_reported(format!("dbus:{BUS} {} ({} interface not exposed for this drive)", self.path, self.name));
            };
            let Some(prop) = props.get(key) else { return Field::not_reported(format!("{} (property not exposed)", self.src(key))) };
            let note = if cached {
                match self.fresh() {
                    Fresh::Never => return Field::not_reported(format!("{} (SmartUpdated=0: SMART never read)", self.src(key))),
                    Fresh::At(t) => format!(" (cached, as of {})", format_utc(t)),
                    Fresh::Unknown => " (cached, SmartUpdated not exposed: age unknown)".into(),
                }
            } else {
                String::new()
            };
            match conv(prop) {
                Ok(Some(v)) => Field::available(v, format!("{}{note}", self.src(key))),
                Ok(None) => Field::not_reported(format!("{} (drive reports unknown)", self.src(key))),
                Err(e) => Field::failed(e, self.src(key)),
            }
        }

        fn last_update(&self, now: u64) -> Field<Value> {
            self.field("SmartUpdated", false, |p| match p.as_u64() {
                Some(0) => Ok(None),
                Some(t) => Ok(Some(Value::Text(format!("{} ({})", format_utc(t), ago(now, t))))),
                None => Err("SmartUpdated is not a timestamp".into()),
            })
        }
    }

    fn temperature(p: &Prop) -> Result<Option<Value>, String> {
        let k = p.as_f64().ok_or("temperature is not a number")?;
        Ok(kelvin_to_celsius(k).map(Value::Celsius))
    }

    fn hours_text(hours: f64) -> Value {
        Value::Text(format!("{:.0} hours ({:.1} days)", hours, hours / 24.0))
    }

    fn number(p: &Prop) -> Result<f64, String> {
        p.as_f64().ok_or_else(|| "not a number".into())
    }

    fn count(p: &Prop) -> Result<Option<Value>, String> {
        match p.as_u64() {
            Some(n) => Ok(Some(Value::Count(n))),
            // UDisks2 uses -1 for "not known" in the ATA counters.
            None if matches!(p, Prop::I64(n) if *n < 0) => Ok(None),
            None => Err("not a count".into()),
        }
    }

    fn text(p: &Prop) -> Result<Option<Value>, String> {
        match p {
            Prop::Str(s) if s.is_empty() => Ok(None),
            Prop::Str(s) => Ok(Some(Value::Text(s.clone()))),
            _ => Err("not a string".into()),
        }
    }

    fn boolean(p: &Prop) -> Result<Option<Value>, String> {
        match p {
            Prop::Bool(b) => Ok(Some(Value::Bool(*b))),
            _ => Err("not a boolean".into()),
        }
    }

    /// Live drive temperature from hwmon. Not read when the PCI device is runtime-suspended.
    fn live_temperature(root: &Path, block: &str) -> Spec {
        let mut dirs: Vec<String> = Vec::new();
        if let Some(rest) = block.strip_prefix("nvme") {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let ctrl = format!("nvme{digits}");
            let status = read_field(root, &format!("sys/class/nvme/{ctrl}/device/power/runtime_status"));
            if status.value().map(String::as_str) == Some("suspended") {
                let src = source(&format!("sys/class/nvme/{ctrl}/hwmon*/temp1_input"));
                return Spec::missing("Temperature (live)", &format!("{src} (not read: device is runtime-suspended)")).live();
            }
            dirs.extend(list_dir(root, &format!("sys/class/nvme/{ctrl}")).into_iter().filter(|e| e.starts_with("hwmon")).map(|e| format!("sys/class/nvme/{ctrl}/{e}")));
        } else {
            dirs.extend(list_dir(root, &format!("sys/block/{block}/device")).into_iter().filter(|e| e.starts_with("hwmon")).map(|e| format!("sys/block/{block}/device/{e}")));
        }
        match dirs.first() {
            Some(d) => Spec::new("Temperature (live)", milli_temp(read_field(root, &format!("{d}/temp1_input")))).live(),
            None => Spec::missing("Temperature (live)", &source(&format!("sys/block/{block}/device/hwmon*/temp1_input"))).live(),
        }
    }

    /// Build the "Health" group for one block device.
    pub fn health_group(root: &Path, block: &str, smart: &dyn SmartSource, now: u64) -> SpecGroup {
        let result = smart.drive_for(block);
        let live = live_temperature(root, block);
        let is_nvme = block.starts_with("nvme");
        let empty = String::new();
        let (path, nvme, ata, blocked): (&String, Option<&Props>, Option<&Props>, Option<String>) = match &result {
            Ok(d) => (&d.path, d.nvme.as_ref(), d.ata.as_ref(), None),
            Err(UdisksError::Unavailable(why)) => (&empty, None, None, Some(format!("dbus:{BUS} (service not available: {why})"))),
            Err(UdisksError::NoDrive) => (&empty, None, None, Some(format!("dbus:{BUS} (no drive object for {block})"))),
            Err(UdisksError::Failed(why)) => (&empty, None, None, Some(format!("dbus:{BUS} (query failed: {why})"))),
        };
        let nvme_set = nvme.is_some() || (ata.is_none() && is_nvme);

        if let Some(src) = blocked {
            // The service could not be queried: every UDisks2-backed spec says why.
            let labels: &[&str] = if nvme_set {
                &["Critical warnings", "Power-on time", "Temperature (cached SMART)", "Self-test status", "Controller state", "Last SMART update"]
            } else {
                &["SMART supported", "SMART enabled", "SMART status failing", "Power-on time", "Temperature (cached SMART)", "Self-test status", "Attributes failing", "Bad sectors", "Attributes failed in the past", "Last SMART update"]
            };
            let failed = matches!(result, Err(UdisksError::Failed(_)));
            let mut v: Vec<Spec> = labels
                .iter()
                .map(|l| if failed { Spec::new(*l, Field::failed("UDisks2 query failed", src.clone())) } else { Spec::missing(l, &src) })
                .collect();
            v.push(live);
            return SpecGroup::new("Health", v);
        }

        let specs = if nvme_set {
            let i = Iface { path, name: NVME, props: nvme };
            vec![
                Spec::new(
                    "Critical warnings",
                    i.field("SmartCriticalWarning", true, |p| match p {
                        Prop::StrList(w) => Ok(Some(Value::Text(critical_warning_text(w)))),
                        _ => Err("not a list".into()),
                    }),
                ),
                Spec::new("Power-on time", i.field("SmartPowerOnHours", true, |p| number(p).map(|h| Some(hours_text(h))))),
                Spec::new("Temperature (cached SMART)", i.field("SmartTemperature", true, temperature)),
                Spec::new("Self-test status", i.field("SmartSelftestStatus", true, text)),
                Spec::new("Controller state", i.field("State", false, text)),
                Spec::new("Last SMART update", i.last_update(now)),
                live,
            ]
        } else {
            let i = Iface { path, name: ATA, props: ata };
            vec![
                Spec::new("SMART supported", i.field("SmartSupported", false, boolean)),
                Spec::new("SMART enabled", i.field("SmartEnabled", false, boolean)),
                Spec::new("SMART status failing", i.field("SmartFailing", true, boolean)),
                Spec::new("Power-on time", i.field("SmartPowerOnSeconds", true, |p| number(p).map(|s| Some(hours_text(s / 3600.0))))),
                Spec::new("Temperature (cached SMART)", i.field("SmartTemperature", true, temperature)),
                Spec::new("Self-test status", i.field("SmartSelftestStatus", true, text)),
                Spec::new("Attributes failing", i.field("SmartNumAttributesFailing", true, count)),
                Spec::new("Bad sectors", i.field("SmartNumBadSectors", true, count)),
                Spec::new("Attributes failed in the past", i.field("SmartNumAttributesFailedInThePast", true, count)),
                Spec::new("Last SMART update", i.last_update(now)),
                live,
            ]
        };
        SpecGroup::new("Health", specs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;
    use facet_core::Availability;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    #[test]
    fn nvme_drive_with_logical_devices_skipped() {
        let fx = Fx::new(&[
            ("sys/block/nvme0n1/device/model", "SAMSUNG TEST   \n"),
            ("sys/block/nvme0n1/device/firmware_rev", "4L1QKXF7\n"),
            ("sys/block/nvme0n1/device/serial", "S0000EXAMPLE0001\n"),
            ("sys/block/nvme0n1/size", "2000409264\n"),
            ("sys/block/nvme0n1/queue/rotational", "0\n"),
            ("sys/block/nvme0n1/removable", "0\n"),
            ("sys/block/dm-0/size", "100\n"),
            ("sys/block/zram0/size", "100\n"),
            ("sys/block/loop3/size", "0\n"),
        ]);
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let n = &v[0];
        assert_eq!(n.id, "storage:nvme0n1");
        assert_eq!(n.title, "SAMSUNG TEST");
        assert_eq!(spec(n, "Capacity").display(), "1.0 TB");
        assert_eq!(spec(n, "Interface").display(), "NVMe");
        assert_eq!(spec(n, "Type").display(), "NVMe solid-state drive");
        assert!(spec(n, "Serial number").identifier);
        assert_eq!(n.subtitle.as_deref(), Some("1.0 TB · NVMe solid-state drive"));
    }

    #[test]
    fn sata_hdd_and_usb_detected_from_symlink_path() {
        let fx = Fx::new(&[
            ("sys/block/sda/device/model", "WDC WD10\n"),
            ("sys/block/sda/device/rev", "01.0\n"),
            ("sys/block/sda/size", "1953525168\n"),
            ("sys/block/sda/queue/rotational", "1\n"),
            ("sys/block/sdb/size", "62552064\n"),
            ("sys/block/sdb/queue/rotational", "0\n"),
        ]);
        // Replace the directories' parents with symlinks is impractical; use link files for transport:
        let fx2 = Fx::new(&[]);
        fx2.symlink("sys/block/sda", "../devices/pci0000:00/0000:00:17.0/ata1/host0/target0:0:0/0:0:0:0/block/sda");
        fx2.symlink("sys/block/sdb", "../devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/host6/target6:0:0/6:0:0:0/block/sdb");
        assert_eq!(transport(fx2.path(), "sda"), Some("SATA"));
        assert_eq!(transport(fx2.path(), "sdb"), Some("USB"));
        let v = collect_from(fx.path());
        assert_eq!(spec(&v[0], "Type").display(), "Hard disk drive");
        assert_eq!(spec(&v[0], "Firmware").display(), "01.0");
        assert_eq!(spec(&v[0], "Interface").display(), "Not reported");
    }

    #[test]
    fn malformed_and_missing_values_degrade_without_panicking() {
        let fx = Fx::new(&[("sys/block/sda/size", "abc\n"), ("sys/block/sda/removable", "7\n")]);
        let v = collect_from(fx.path());
        let c = &v[0];
        assert_eq!(c.title, "Storage device sda");
        assert!(matches!(spec(c, "Capacity").value.availability, Availability::Failed { .. }));
        assert!(matches!(spec(c, "Removable").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Model").display(), "Not reported");
    }

    #[test]
    fn unreadable_root_yields_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    // ---- health -----------------------------------------------------------------------------------

    use super::health::{DriveInfo, Prop, Props, SmartSource, UdisksError, critical_warning_text, format_utc, kelvin_to_celsius};
    use facet_core::Availability as Av;

    struct Canned(Result<DriveInfo, UdisksError>);

    impl SmartSource for Canned {
        fn drive_for(&self, _: &str) -> Result<DriveInfo, UdisksError> {
            self.0.clone()
        }
    }

    fn props(v: &[(&str, Prop)]) -> Props {
        v.iter().map(|(k, p)| ((*k).to_owned(), p.clone())).collect()
    }

    const NOW: u64 = 1_791_609_800;

    fn nvme_fx() -> Fx {
        Fx::new(&[
            ("sys/block/nvme0n1/device/model", "SAMSUNG TEST\n"),
            ("sys/block/nvme0n1/size", "2000409264\n"),
            ("sys/class/nvme/nvme0/hwmon2/temp1_input", "38900\n"),
        ])
    }

    fn health_of(root: &Path, smart: &dyn SmartSource) -> Vec<Spec> {
        let v = collect_with(root, smart, NOW);
        v[0].groups.iter().find(|g| g.title == "Health").expect("Health group").specs.clone()
    }

    fn find<'a>(specs: &'a [Spec], label: &str) -> &'a Spec {
        specs.iter().find(|s| s.label == label).unwrap_or_else(|| panic!("no {label}"))
    }

    #[test]
    fn nvme_health_from_udisks_properties() {
        let fx = nvme_fx();
        let info = DriveInfo {
            path: "/org/freedesktop/UDisks2/drives/X".into(),
            nvme: Some(props(&[
                ("SmartCriticalWarning", Prop::StrList(vec![])),
                ("SmartPowerOnHours", Prop::U64(902)),
                ("SmartTemperature", Prop::U64(311)),
                ("SmartUpdated", Prop::U64(1_791_609_673)),
                ("SmartSelftestStatus", Prop::Str("success".into())),
                ("State", Prop::Str("live".into())),
            ])),
            ata: None,
        };
        let h = health_of(fx.path(), &Canned(Ok(info)));
        assert_eq!(find(&h, "Critical warnings").display(), "None reported");
        let src = &find(&h, "Critical warnings").value.source;
        assert!(src.contains("as of 2026-10-10 05:21 UTC"), "{src}");
        assert_eq!(find(&h, "Power-on time").display(), "902 hours (37.6 days)");
        let t = find(&h, "Temperature (cached SMART)");
        assert!(matches!(t.value.value(), Some(Value::Celsius(c)) if (c - 37.85).abs() < 1e-9));
        assert!(!t.live, "cached temperature is not live");
        assert_eq!(find(&h, "Self-test status").display(), "success");
        assert_eq!(find(&h, "Controller state").display(), "live");
        assert!(find(&h, "Last SMART update").display().contains("2 min ago"));
        let live = find(&h, "Temperature (live)");
        assert_eq!(live.value.value(), Some(&Value::Celsius(38.9)));
        assert!(live.live);
        assert!(live.value.source.starts_with("sysfs:/sys/class/nvme/nvme0/hwmon2"));
    }

    #[test]
    fn critical_warnings_are_listed_not_hidden() {
        assert_eq!(critical_warning_text(&[]), "None reported");
        assert_eq!(critical_warning_text(&["temperature".into(), "media-read-only".into()]), "temperature, media-read-only");
        let fx = nvme_fx();
        let info = DriveInfo {
            path: "/p".into(),
            nvme: Some(props(&[("SmartCriticalWarning", Prop::StrList(vec!["spare".into()])), ("SmartUpdated", Prop::U64(5))])),
            ata: None,
        };
        assert_eq!(find(&health_of(fx.path(), &Canned(Ok(info))), "Critical warnings").display(), "spare");
    }

    #[test]
    fn kelvin_conversion() {
        assert_eq!(kelvin_to_celsius(273.15), Some(0.0));
        assert!((kelvin_to_celsius(311.0).unwrap() - 37.85).abs() < 1e-9);
        assert_eq!(kelvin_to_celsius(0.0), None);
        assert_eq!(format_utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(format_utc(1_791_609_673), "2026-10-10 05:21 UTC");
    }

    #[test]
    fn never_updated_smart_is_not_reported() {
        let fx = nvme_fx();
        let info = DriveInfo {
            path: "/p".into(),
            nvme: Some(props(&[
                ("SmartCriticalWarning", Prop::StrList(vec![])),
                ("SmartPowerOnHours", Prop::U64(0)),
                ("SmartTemperature", Prop::U64(0)),
                ("SmartUpdated", Prop::U64(0)),
            ])),
            ata: None,
        };
        let h = health_of(fx.path(), &Canned(Ok(info)));
        for l in ["Critical warnings", "Power-on time", "Temperature (cached SMART)", "Last SMART update"] {
            assert!(matches!(find(&h, l).value.availability, Av::NotReported), "{l}");
        }
        assert!(find(&h, "Critical warnings").value.source.contains("SmartUpdated=0"));
        // live hwmon is independent of the stale SMART cache
        assert!(find(&h, "Temperature (live)").is_available());
    }

    #[test]
    fn missing_interface_and_zero_kelvin() {
        let fx = nvme_fx();
        let h = health_of(fx.path(), &Canned(Ok(DriveInfo { path: "/p".into(), nvme: None, ata: None })));
        let c = find(&h, "Critical warnings");
        assert!(matches!(c.value.availability, Av::NotReported));
        assert!(c.value.source.contains("interface not exposed"));
        let info = DriveInfo { path: "/p".into(), nvme: Some(props(&[("SmartTemperature", Prop::U64(0)), ("SmartUpdated", Prop::U64(9))])), ata: None };
        let h = health_of(fx.path(), &Canned(Ok(info)));
        assert!(matches!(find(&h, "Temperature (cached SMART)").value.availability, Av::NotReported));
        assert!(matches!(find(&h, "Power-on time").value.availability, Av::NotReported), "absent property");
    }

    #[test]
    fn service_unavailable_degrades_cleanly() {
        let fx = nvme_fx();
        let h = health_of(fx.path(), &Canned(Err(UdisksError::Unavailable("no bus".into()))));
        let c = find(&h, "Critical warnings");
        assert!(matches!(c.value.availability, Av::NotReported));
        assert_eq!(c.value.source, "dbus:org.freedesktop.UDisks2 (service not available: no bus)");
        assert_eq!(find(&h, "Temperature (live)").display(), "39 °C");
        let h = health_of(fx.path(), &Canned(Err(UdisksError::Failed("timeout".into()))));
        assert!(matches!(find(&h, "Critical warnings").value.availability, Av::Failed { .. }));
        let h = health_of(fx.path(), &Canned(Err(UdisksError::NoDrive)));
        assert!(find(&h, "Critical warnings").value.source.contains("no drive object"));
        // default fixture entry point never touches the live bus
        let v = collect_from(fx.path());
        assert!(v[0].groups.iter().any(|g| g.title == "Health"));
    }

    #[test]
    fn ata_health_and_unknown_counters() {
        let fx = Fx::new(&[("sys/block/sda/size", "1000\n"), ("sys/block/sda/device/hwmon3/temp1_input", "-273000\n")]);
        let info = DriveInfo {
            path: "/org/freedesktop/UDisks2/drives/S".into(),
            nvme: None,
            ata: Some(props(&[
                ("SmartSupported", Prop::Bool(true)),
                ("SmartEnabled", Prop::Bool(true)),
                ("SmartFailing", Prop::Bool(false)),
                ("SmartPowerOnSeconds", Prop::U64(7_200_000)),
                ("SmartTemperature", Prop::F64(300.15)),
                ("SmartUpdated", Prop::U64(1_791_609_000)),
                ("SmartNumAttributesFailing", Prop::I64(0)),
                ("SmartNumBadSectors", Prop::I64(-1)),
                ("SmartNumAttributesFailedInThePast", Prop::I64(2)),
            ])),
        };
        let h = health_of(fx.path(), &Canned(Ok(info)));
        assert_eq!(find(&h, "SMART status failing").value.value(), Some(&Value::Bool(false)));
        assert_eq!(find(&h, "Power-on time").display(), "2000 hours (83.3 days)");
        assert!(matches!(find(&h, "Temperature (cached SMART)").value.value(), Some(Value::Celsius(c)) if (c - 27.0).abs() < 1e-9));
        assert_eq!(find(&h, "Attributes failing").value.value(), Some(&Value::Count(0)));
        assert!(matches!(find(&h, "Bad sectors").value.availability, Av::NotReported));
        assert_eq!(find(&h, "Attributes failed in the past").value.value(), Some(&Value::Count(2)));
        assert!(matches!(find(&h, "Temperature (live)").value.availability, Av::Failed { .. }), "absurd hwmon temperature");
    }

    #[test]
    fn suspended_nvme_hwmon_is_not_read() {
        let fx = nvme_fx();
        fx.write("sys/class/nvme/nvme0/device/power/runtime_status", "suspended\n");
        let h = health_of(fx.path(), &Canned(Err(UdisksError::NoDrive)));
        let l = find(&h, "Temperature (live)");
        assert!(matches!(l.value.availability, Av::NotReported));
        assert!(l.value.source.contains("runtime-suspended"));
    }

    /// Live-machine check: `cargo test -p facet-collectors -- --ignored --nocapture live_`
    #[test]
    #[ignore]
    fn live_storage_components() {
        for c in collect() {
            println!("{} | {} | {:?}", c.id, c.title, c.subtitle);
            for g in c.groups.iter().filter(|g| g.title == "Health") {
                for s in &g.specs {
                    println!("   {:34} {:40} [{}]", s.label, s.display(), s.value.source);
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn live_power_and_sensors_components() {
        for c in crate::power::collect().into_iter().chain(crate::sensors::collect()) {
            println!("{} | {} | {:?} | parent {:?}", c.id, c.title, c.subtitle, c.parent);
            for s in c.summary.iter() {
                println!("   * {:34} {}", s.label, s.display());
            }
            for g in &c.groups {
                println!("  [{}]", g.title);
                for s in &g.specs {
                    println!("   {:34} {:30} [{}]", s.label, s.display(), s.value.source);
                }
            }
        }
    }
}
