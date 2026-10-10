//! Realistic fictional machines for UI work, tests and demos (`facet-app --fixture laptop`).
//! Values are plausible, not measurements of any real device.

use crate::{Category, Component, Field, Inventory, Spec, SpecGroup, Value};

const SRC: &str = "fixture";

fn t(label: &str, v: &str) -> Spec {
    Spec::text(label, v, SRC)
}
fn v(label: &str, val: Value) -> Spec {
    Spec::val(label, val, SRC)
}
fn missing(label: &str) -> Spec {
    Spec::missing(label, SRC)
}
fn locked(label: &str) -> Spec {
    Spec::new(label, Field::permission_required(SRC))
}
fn sleeping(label: &str) -> Spec {
    Spec::new(label, Field::not_reported("dGPU runtime-suspended; not queried to avoid waking it")).live()
}
const GIB: u64 = 1 << 30;

fn system(vendor: &str, model: &str, chassis: &str, bios: &str, vm: bool) -> Component {
    Component::new("system:machine", Category::System, &format!("{vendor} {model}"))
        .subtitle(chassis)
        .summary(vec![t("Manufacturer", vendor), t("Model", model), t("Chassis", chassis)])
        .group(SpecGroup::new("Firmware", vec![t("BIOS version", bios), t("Firmware mode", "UEFI"), t("Virtual machine", if vm { "Yes (KVM)" } else { "No" })]))
        .group(SpecGroup::new("Identity", vec![locked("Serial number").identifier(), locked("UUID").identifier()]))
        .group(SpecGroup::new("Operating system", vec![t("Kernel", "7.2.5-3-omarchy")]).technical())
}

fn cpu(name: &str, cores: u64, threads: u64, max_ghz: u64, l3: u64) -> Component {
    Component::new("cpu:package0", Category::Processor, name)
        .subtitle(&format!("{cores} cores · {threads} threads"))
        .summary(vec![v("Cores", Value::Count(cores)), v("Threads", Value::Count(threads)), v("Max frequency", Value::Hertz(max_ghz * 100_000_000)).rated()])
        .group(SpecGroup::new("Frequency", vec![
            v("Current", Value::Hertz(1_200_000_000)).live(),
            v("Minimum", Value::Hertz(800_000_000)).rated(),
            v("Maximum", Value::Hertz(max_ghz * 100_000_000)).rated(),
        ]))
        .group(SpecGroup::new("Cache", vec![v("L1 data", Value::BinaryBytes(48 * 1024 * cores)), v("L2", Value::BinaryBytes(2 * 1024 * 1024 * cores / 2)), v("L3", Value::BinaryBytes(l3 * 1024 * 1024))]))
        .group(SpecGroup::new("Instruction sets", vec![t("Notable", "AVX2, AES-NI, SSE4.2, VT-x, SHA")]).technical())
        .group(SpecGroup::new("Security mitigations", vec![t("Spectre v2", "Enhanced IBRS"), t("Meltdown", "Not affected")]).technical())
}

fn memory(total_gb: u64, free_pct: f64) -> Component {
    Component::new("memory:system", Category::Memory, &format!("{total_gb} GiB system memory"))
        .subtitle("DDR5")
        .summary(vec![v("Installed", Value::BinaryBytes(total_gb * GIB)), v("In use", Value::Percent(100.0 - free_pct)).live(), v("Type", Value::Text("DDR5".into()))])
        .group(SpecGroup::new("Capacity", vec![v("Available", Value::BinaryBytes(total_gb * GIB * (free_pct as u64) / 100)).live(), v("Swap", Value::BinaryBytes(8 * GIB))]))
        .group(SpecGroup::new("Modules", vec![locked("Module detail")]).technical())
}

fn dimm(n: u32, gb: u64, speed: u64) -> Component {
    Component::new(&format!("memory:dimm{n}"), Category::Memory, &format!("{gb} GB DDR5-{speed}"))
        .subtitle(&format!("Slot {n}"))
        .parent("memory:system")
        .summary(vec![v("Size", Value::BinaryBytes(gb * GIB)), t("Speed", &format!("{speed} MT/s")), t("Manufacturer", "Micron")])
        .group(SpecGroup::new("Identity", vec![t("Part number", "CT16G56C46S5.M8G1"), t("Serial number", "0xA1B2C3D4").identifier()]))
}

fn gpu(id: &str, name: &str, discrete: bool, asleep: bool, driver: &str) -> Component {
    let live = if asleep {
        vec![sleeping("Temperature"), sleeping("Memory in use")]
    } else {
        vec![v("Temperature", Value::Celsius(48.0)).live(), v("Memory in use", Value::Percent(12.0)).live()]
    };
    Component::new(id, Category::Graphics, name)
        .subtitle(if discrete { "Discrete GPU" } else { "Integrated GPU · primary display" })
        .summary(vec![t("Driver", driver), t("Role", if discrete { "Discrete" } else { "Integrated" }), t("Power state", if asleep { "Suspended" } else { "Active" })])
        .group(SpecGroup::new("Live readings", live))
        .group(SpecGroup::new("Connection", vec![t("Bus address", id.trim_start_matches("graphics:")), t("PCIe link", "16 GT/s x8 (rated 32 GT/s x8)")]))
}

fn nvme(name: &str, model: &str, tb: f64, hours: u64, temp: f64) -> Component {
    Component::new(&format!("storage:{name}"), Category::Storage, model)
        .subtitle(&format!("{} · NVMe solid-state drive", crate::format_bytes((tb * 1e12) as u64)))
        .summary(vec![v("Capacity", Value::Bytes((tb * 1e12) as u64)), t("Type", "NVMe solid-state drive"), t("Interface", "NVMe")])
        .group(SpecGroup::new("Health", vec![
            t("Critical warnings", "None reported"),
            v("Power-on time", Value::Count(hours)),
            v("Temperature", Value::Celsius(temp)).live(),
            t("Data as of", "2 minutes ago"),
        ]))
        .group(SpecGroup::new("Identity", vec![t("Model", model), t("Firmware", "4L1QKXF7"), t("Serial number", "S0000EXAMPLE0001").identifier()]))
        .group(SpecGroup::new("Device", vec![t("Kernel name", name), v("Removable", Value::Bool(false))]).technical())
}

fn sata(name: &str, model: &str, tb: f64, hdd: bool) -> Component {
    let kind = if hdd { "Hard disk drive" } else { "Solid-state drive" };
    Component::new(&format!("storage:{name}"), Category::Storage, model)
        .subtitle(&format!("{} · {kind}", crate::format_bytes((tb * 1e12) as u64)))
        .summary(vec![v("Capacity", Value::Bytes((tb * 1e12) as u64)), t("Type", kind), t("Interface", "SATA")])
        .group(SpecGroup::new("Health", vec![missing("SMART status")]))
        .group(SpecGroup::new("Identity", vec![t("Model", model), t("Serial number", "WD-WCC4E1234567").identifier()]))
}

fn nic(id: &str, title: &str, kind: &str, link: &str, mac: &str, parent: Option<&str>) -> Component {
    let mut c = Component::new(id, Category::Network, title)
        .subtitle(kind)
        .summary(vec![t("Type", kind), t("Link", link), v("Speed", Value::Bps(if kind == "Wi-Fi" { 2_401_000_000 } else { 2_500_000_000 })).live()])
        .group(SpecGroup::new("Identity", vec![t("MAC address", mac).identifier(), t("Driver", if kind == "Wi-Fi" { "mt7925e" } else { "r8169" })]));
    if let Some(p) = parent {
        c = c.parent(p);
    }
    c
}

fn display(id: &str, title: &str, size: &str, res: &str, hz: u64, inches: f64, gpu: &str) -> Component {
    Component::new(id, Category::Displays, title)
        .subtitle(&format!("{res} · {hz} Hz"))
        .parent(gpu)
        .summary(vec![t("Resolution", res), v("Refresh rate", Value::Hertz(hz)), t("Size", &format!("{inches:.1}\" ({size})"))])
        .group(SpecGroup::new("Panel", vec![t("Manufacturer", "Lenovo Group Limited"), t("Model", title), t("Serial number", "0x00000001").identifier()]))
        .group(SpecGroup::new("EDID", vec![t("Version", "1.4"), t("Colour depth", "10 bits")]).technical())
}

fn usb(id: &str, title: &str, sub: &str, parent: Option<&str>, speed: Value) -> Component {
    let mut c = Component::new(id, Category::Peripherals, title)
        .subtitle(sub)
        .summary(vec![v("Speed", speed), t("Bus", "USB")])
        .group(SpecGroup::new("Identity", vec![t("Product ID", "046d:c52b"), t("Serial number", "0000AB12").identifier()]).technical());
    if let Some(p) = parent {
        c = c.parent(p);
    }
    c
}

fn battery(health_pct: f64, cycles: u64) -> Vec<Component> {
    vec![
        Component::new("power:BAT0", Category::Power, "Internal battery")
            .subtitle("80 Wh design capacity")
            .summary(vec![v("Charge", Value::Percent(86.0)).live(), t("Status", "Discharging").live(), v("Health", Value::Percent(health_pct))])
            .group(SpecGroup::new("Capacity", vec![v("Design", Value::Watts(80.0)), v("Full charge now", Value::Watts(80.0 * health_pct / 100.0)), v("Cycle count", Value::Count(cycles))]))
            .group(SpecGroup::new("Identity", vec![t("Manufacturer", "SMP"), t("Serial number", "1234").identifier()])),
        Component::new("power:AC", Category::Power, "AC adapter").summary(vec![t("Connected", "No").live()]),
    ]
}

fn sensors(cpu_temp: f64, fan: u64) -> Vec<Component> {
    vec![
        Component::new("sensors:coretemp", Category::Power, "CPU package temperature")
            .subtitle("coretemp")
            .summary(vec![v("Package", Value::Celsius(cpu_temp)).live(), v("Critical", Value::Celsius(100.0)).rated()])
            .group(SpecGroup::new("Cores", vec![v("Core 0", Value::Celsius(cpu_temp - 3.0)).live(), v("Core 8", Value::Celsius(cpu_temp - 5.0)).live()]).technical()),
        Component::new("sensors:fans", Category::Power, "Cooling fans")
            .subtitle("Embedded controller")
            .summary(vec![v("Fan 1", Value::Rpm(fan)).live(), v("Fan 2", Value::Rpm(fan + 40)).live()]),
    ]
}

pub fn laptop() -> Inventory {
    let mut c = vec![
        system("Lenovo", "Legion 5 15IRX10", "Notebook", "QNCN35WW (11/28/2025)", false),
        cpu("Intel Core i9-14900HX", 24, 32, 58, 36),
        memory(32, 62.0),
        dimm(0, 16, 5600),
        dimm(1, 16, 5600),
        gpu("graphics:0000:00:02.0", "Intel Raptor Lake-S UHD Graphics", false, false, "i915"),
        gpu("graphics:0000:01:00.0", "NVIDIA GeForce RTX 5070 Max-Q / Mobile", true, true, "nvidia"),
        nvme("nvme0n1", "SAMSUNG MZAL81T0HFLB-00BL2", 1.0, 899, 43.0),
        nic("network:wlan0", "MediaTek MT7925 Wi-Fi 7", "Wi-Fi", "Connected", "A4:B1:C1:22:33:44", Some("pci:0000:03:00.0")),
        nic("network:enp4s0", "Realtek RTL8125 2.5GbE", "Ethernet", "No cable", "A4:B1:C1:22:33:45", Some("pci:0000:04:00.0")),
        display("display:card1-eDP-1", "Built-in display", "34 × 19 cm", "2560 × 1600", 165, 16.0, "graphics:0000:00:02.0"),
        display("display:card1-DP-3", "DELL U3425WE", "80 × 34 cm", "3440 × 1440", 60, 34.1, "graphics:0000:00:02.0"),
        Component::new("pci:0000:03:00.0", Category::Peripherals, "MediaTek MT7925 Wi-Fi controller").subtitle("PCI Express").summary(vec![t("Driver", "mt7925e"), t("Link", "PCIe 8 GT/s x1")]),
        Component::new("pci:0000:04:00.0", Category::Peripherals, "Realtek RTL8125 Ethernet controller").subtitle("PCI Express").summary(vec![t("Driver", "r8169"), t("Link", "PCIe 5 GT/s x1")]),
        usb("usb:usb3", "xHCI Host Controller", "USB 3.1 root hub", None, Value::Bps(10_000_000_000)),
        usb("usb:3-1", "Dell Thunderbolt Dock WD22TB4", "USB hub", Some("usb:usb3"), Value::Bps(10_000_000_000)),
        usb("usb:3-1.2", "Logitech BRIO webcam", "Camera", Some("usb:3-1"), Value::Bps(5_000_000_000)),
        usb("usb:3-1.3", "Keychron K3 keyboard", "Keyboard", Some("usb:3-1"), Value::Bps(12_000_000)),
        usb("usb:3-1.4", "Logitech USB Receiver", "Mouse receiver", Some("usb:3-1"), Value::Bps(12_000_000)),
        Component::new("bluetooth:hci0", Category::Peripherals, "Bluetooth adapter").subtitle("Bluetooth 5.4").summary(vec![t("Version", "5.4"), t("Address", "A4:B1:C1:22:33:46").identifier()]),
        Component::new("audio:card0", Category::Peripherals, "HDA Intel PCH — Realtek ALC287").subtitle("Audio").summary(vec![t("Driver", "snd_hda_intel")]),
    ];
    c.extend(battery(94.0, 112));
    c.extend(sensors(52.0, 2300));
    Inventory { components: c }
}

pub fn desktop() -> Inventory {
    let mut c = vec![
        system("ASUS", "ROG STRIX B650E-F GAMING WIFI", "Desktop", "3002 (06/03/2025)", false),
        cpu("AMD Ryzen 9 7950X", 16, 32, 57, 64),
        memory(64, 71.0),
        dimm(0, 32, 6000),
        dimm(2, 32, 6000),
        gpu("graphics:0000:0a:00.0", "AMD Radeon RX 7900 XTX", true, false, "amdgpu"),
        nvme("nvme0n1", "WD_BLACK SN850X 2000GB", 2.0, 4120, 38.0),
        sata("sda", "WDC WD40EFRX-68N32N0", 4.0, true),
        sata("sdb", "Samsung SSD 870 EVO 1TB", 1.0, false),
        nic("network:enp5s0", "Intel I225-V 2.5GbE", "Ethernet", "Connected", "58:11:22:33:44:55", None),
        display("display:card1-DP-1", "LG ULTRAGEAR 27GR95QE", "60 × 34 cm", "2560 × 1440", 240, 26.5, "graphics:0000:0a:00.0"),
        display("display:card1-DP-2", "Dell U2723QE", "60 × 34 cm", "3840 × 2160", 60, 27.0, "graphics:0000:0a:00.0"),
        display("display:card1-HDMI-A-1", "Samsung Q80T", "122 × 69 cm", "3840 × 2160", 60, 55.0, "graphics:0000:0a:00.0"),
        usb("usb:usb1", "xHCI Host Controller", "USB 2.0 root hub", None, Value::Bps(480_000_000)),
        usb("usb:1-4", "Razer Basilisk V3", "Mouse", Some("usb:usb1"), Value::Bps(12_000_000)),
    ];
    c.extend(sensors(41.0, 1100));
    Inventory { components: c }
}

pub fn vm() -> Inventory {
    Inventory {
        components: vec![
            system("QEMU", "Standard PC (Q35 + ICH9, 2009)", "Other", "1.16.3 (04/01/2014)", true),
            cpu("Intel Xeon Processor (Sapphire Rapids)", 4, 4, 35, 16),
            memory(8, 80.0),
            gpu("graphics:0000:00:01.0", "Red Hat Virtio 1.0 GPU", false, false, "virtio-pci"),
            sata("vda", "QEMU HARDDISK", 0.1, false),
            nic("network:enp1s0", "Red Hat Virtio network device", "Ethernet", "Connected", "52:54:00:12:34:56", None),
            display("display:card0-Virtual-1", "Virtual display", "—", "1920 × 1080", 60, 0.0, "graphics:0000:00:01.0"),
        ],
    }
}

pub fn by_name(name: &str) -> Option<Inventory> {
    match name {
        "laptop" => Some(laptop()),
        "desktop" => Some(desktop()),
        "vm" => Some(vm()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn fixtures_are_well_formed() {
        for name in ["laptop", "desktop", "vm"] {
            let inv = by_name(name).unwrap();
            let ids: HashSet<_> = inv.components.iter().map(|c| c.id.as_str()).collect();
            assert_eq!(ids.len(), inv.components.len(), "{name}: duplicate ids");
            for c in &inv.components {
                if let Some(p) = &c.parent {
                    assert!(ids.contains(p.as_str()), "{name}: {} has dangling parent {p}", c.id);
                }
                assert!(!c.title.is_empty());
            }
        }
    }

    #[test]
    fn laptop_has_dock_tree_and_sleeping_dgpu() {
        let inv = laptop();
        assert_eq!(inv.children_of("usb:3-1").count(), 3);
        let d = inv.get("graphics:0000:01:00.0").unwrap();
        let temp = d.all_specs().find(|s| s.label == "Temperature").unwrap();
        assert!(!temp.is_available());
    }
}
