//! The machine itself: DMI/SMBIOS identity, firmware mode, kernel and virtualization.

use std::path::Path;

use facet_core::{Availability, Category, Component, Field, Spec, SpecGroup, Value};

use crate::util::{exists, parse_field, read_field, source, text_spec};

pub fn collect() -> Vec<Component> {
    collect_from(Path::new("/"))
}

const DMI: &str = "sys/class/dmi/id";

fn dmi(root: &Path, name: &str) -> Field<String> {
    read_field(root, &format!("{DMI}/{name}"))
}

/// Firmware vendors fill unset DMI strings with placeholders instead of leaving them empty.
fn meaningful(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    !(l.is_empty()
        || ["none", "default string", "not specified", "not applicable", "system version", "system product name", "to be filled by o.e.m."]
            .contains(&l.as_str()))
}

/// SMBIOS chassis type (DSP0134) as a name. Bit 7 is the chassis-lock flag and is ignored.
pub fn chassis_name(code: u8) -> Option<&'static str> {
    Some(match code & 0x7f {
        1 => "Other",
        2 => "Unknown",
        3 => "Desktop",
        4 => "Low-profile desktop",
        5 => "Pizza box",
        6 => "Mini tower",
        7 => "Tower",
        8 => "Portable",
        9 => "Laptop",
        10 => "Notebook",
        11 => "Handheld",
        12 => "Docking station",
        13 => "All-in-one",
        14 => "Sub-notebook",
        15 => "Space-saving",
        16 => "Lunch box",
        17 => "Server",
        18 => "Expansion chassis",
        19 => "Sub-chassis",
        20 => "Bus expansion chassis",
        21 => "Peripheral chassis",
        22 => "RAID chassis",
        23 => "Rack-mount",
        24 => "Sealed-case PC",
        25 => "Multi-system chassis",
        26 => "CompactPCI",
        27 => "AdvancedTCA",
        28 => "Blade",
        29 => "Blade enclosure",
        30 => "Tablet",
        31 => "Convertible",
        32 => "Detachable",
        33 => "IoT gateway",
        34 => "Embedded PC",
        35 => "Mini PC",
        36 => "Stick PC",
        _ => return None,
    })
}

/// Hypervisor named by DMI vendor/product strings, if any.
pub fn hypervisor_from_dmi(vendor: &str, product: &str) -> Option<&'static str> {
    let hay = format!("{vendor} {product}").to_lowercase();
    const TABLE: [(&str, &str); 10] = [
        ("vmware", "VMware"),
        ("virtualbox", "VirtualBox"),
        ("innotek", "VirtualBox"),
        ("qemu", "QEMU/KVM"),
        ("kvm", "KVM"),
        ("xen", "Xen"),
        ("parallels", "Parallels"),
        ("bochs", "Bochs"),
        ("google compute engine", "Google Compute Engine"),
        ("amazon ec2", "Amazon EC2"),
    ];
    for (needle, name) in TABLE {
        if hay.contains(needle) {
            return Some(name);
        }
    }
    // "Microsoft Corporation / Virtual Machine" is Hyper-V; plain Microsoft hardware (Surface) is not.
    if hay.contains("microsoft") && hay.contains("virtual machine") {
        return Some("Microsoft Hyper-V");
    }
    None
}

fn cpuinfo_hypervisor_flag(root: &Path) -> Field<bool> {
    let f = read_field(root, "proc/cpuinfo");
    let src = "procfs:/proc/cpuinfo".to_owned();
    match f.availability {
        Availability::Available { value } => match value.lines().find(|l| l.starts_with("flags")) {
            Some(l) => Field::available(l.split_whitespace().any(|w| w == "hypervisor"), src),
            None => Field::not_reported(src),
        },
        Availability::PermissionRequired => Field::permission_required(src),
        Availability::Failed { reason } => Field::failed(reason, src),
        _ => Field::not_reported(src),
    }
}

fn virtualization(root: &Path) -> Vec<Spec> {
    let vendor = dmi(root, "sys_vendor");
    let product = dmi(root, "product_name");
    let dmi_hit = hypervisor_from_dmi(vendor.value().map_or("", String::as_str), product.value().map_or("", String::as_str));
    let flag = cpuinfo_hypervisor_flag(root);
    let flag_src = flag.source.clone();
    let dmi_src = source(&format!("{DMI}/sys_vendor"));
    match (dmi_hit, flag.value().copied()) {
        (Some(name), _) => vec![Spec::val("Virtual machine", Value::Bool(true), &dmi_src), Spec::text("Hypervisor", name, &dmi_src)],
        (None, Some(true)) => vec![
            Spec::val("Virtual machine", Value::Bool(true), &flag_src),
            Spec::text("Hypervisor", "Detected by CPU flag; vendor not identified", &flag_src),
        ],
        (None, Some(false)) => vec![Spec::val("Virtual machine", Value::Bool(false), &flag_src)],
        (None, None) => vec![Spec::new("Virtual machine", parse_field(flag, |b| Ok(Value::Bool(*b))))],
    }
}

pub fn collect_from(root: &Path) -> Vec<Component> {
    let manufacturer = dmi(root, "sys_vendor");
    let product_name = dmi(root, "product_name");
    let product_version = dmi(root, "product_version");
    let family = dmi(root, "product_family");
    let kernel = {
        let mut k = read_field(root, "proc/sys/kernel/osrelease");
        k.source = "procfs:/proc/sys/kernel/osrelease".into();
        k
    };

    if !exists(root, DMI) && kernel.value().is_none() {
        return Vec::new();
    }

    let title = [&family, &product_version, &product_name]
        .iter()
        .filter_map(|f| f.value())
        .find(|s| meaningful(s))
        .cloned()
        .unwrap_or_else(|| "This computer".to_owned());
    let maker = manufacturer.value().filter(|s| meaningful(s)).cloned();
    let model = product_name.value().filter(|s| meaningful(s)).cloned();
    let subtitle = match (&maker, &model) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (Some(a), None) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        _ => None,
    };

    let chassis = parse_field(dmi(root, "chassis_type"), |s| {
        let code: u8 = s.parse().map_err(|_| format!("unexpected chassis type {s:?}"))?;
        Ok(Value::Text(chassis_name(code).map_or_else(|| format!("Unknown ({code})"), str::to_owned)))
    });

    let efi_src = source("sys/firmware/efi");
    let firmware_mode = if exists(root, "sys/firmware/efi") {
        Spec::text("Firmware mode", "UEFI", &efi_src)
    } else if exists(root, "sys/firmware") {
        Spec::text("Firmware mode", "BIOS (legacy)", &efi_src)
    } else {
        Spec::missing("Firmware mode", &efi_src)
    };

    let mut summary = vec![text_spec("Manufacturer", manufacturer), text_spec("Model", product_name), Spec::new("Chassis", chassis), firmware_mode.clone()];
    summary.extend(virtualization(root));

    let mut c = Component::new("system:machine", Category::System, &title).summary(summary);
    c.subtitle = subtitle;
    vec![c
        .group(SpecGroup::new("Product", vec![
            text_spec("Product family", family),
            text_spec("Product version", product_version),
            text_spec("SKU", dmi(root, "product_sku")),
        ]))
        .group(SpecGroup::new("Mainboard", vec![
            text_spec("Board vendor", dmi(root, "board_vendor")),
            text_spec("Board name", dmi(root, "board_name")),
            text_spec("Board version", dmi(root, "board_version")),
        ]))
        .group(SpecGroup::new("Firmware (BIOS/UEFI)", vec![
            text_spec("BIOS vendor", dmi(root, "bios_vendor")),
            text_spec("BIOS version", dmi(root, "bios_version")),
            text_spec("BIOS date", dmi(root, "bios_date")),
            firmware_mode,
        ]))
        .group(SpecGroup::new("Operating system", vec![text_spec("Kernel version", kernel)]))
        .group(
            SpecGroup::new("Identifiers (some need elevated access)", vec![
                text_spec("System serial number", dmi(root, "product_serial")).identifier(),
                text_spec("Board serial number", dmi(root, "board_serial")).identifier(),
                text_spec("Chassis serial number", dmi(root, "chassis_serial")).identifier(),
                text_spec("System UUID", dmi(root, "product_uuid")).identifier(),
            ])
            .technical(),
        )]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::testutil::Fx;

    fn spec<'a>(c: &'a Component, label: &str) -> &'a Spec {
        c.all_specs().find(|s| s.label == label).unwrap_or_else(|| panic!("no spec {label}"))
    }

    fn legion() -> Fx {
        Fx::new(&[
            ("sys/class/dmi/id/sys_vendor", "LENOVO\n"),
            ("sys/class/dmi/id/product_name", "83LY\n"),
            ("sys/class/dmi/id/product_version", "Legion 5 15IRX10\n"),
            ("sys/class/dmi/id/product_family", "Legion 5 15IRX10\n"),
            ("sys/class/dmi/id/board_vendor", "LENOVO\n"),
            ("sys/class/dmi/id/board_name", "LNVNB161216\n"),
            ("sys/class/dmi/id/board_version", "SDK0T76461 WIN\n"),
            ("sys/class/dmi/id/bios_vendor", "LENOVO\n"),
            ("sys/class/dmi/id/bios_version", "QNCN35WW\n"),
            ("sys/class/dmi/id/bios_date", "11/28/2025\n"),
            ("sys/class/dmi/id/chassis_type", "10\n"),
            ("sys/firmware/efi/efivars/x", ""),
            ("proc/sys/kernel/osrelease", "7.2.5-3-omarchy\n"),
            ("proc/cpuinfo", "processor\t: 0\nflags\t\t: fpu vme sse4_2 avx2 vmx\n"),
        ])
    }

    #[test]
    fn real_machine_fixture() {
        let fx = legion();
        let v = collect_from(fx.path());
        assert_eq!(v.len(), 1);
        let c = &v[0];
        assert_eq!(c.id, "system:machine");
        assert_eq!(c.category, Category::System);
        assert_eq!(c.title, "Legion 5 15IRX10");
        assert_eq!(c.subtitle.as_deref(), Some("LENOVO 83LY"));
        assert_eq!(spec(c, "Chassis").display(), "Notebook");
        assert_eq!(spec(c, "Firmware mode").display(), "UEFI");
        assert_eq!(spec(c, "Kernel version").display(), "7.2.5-3-omarchy");
        assert_eq!(spec(c, "BIOS date").display(), "11/28/2025");
        assert_eq!(spec(c, "BIOS version").display(), "QNCN35WW");
        assert_eq!(spec(c, "Virtual machine").display(), "No");
        assert_eq!(spec(c, "BIOS version").value.source, "sysfs:/sys/class/dmi/id/bios_version");
        assert_eq!(spec(c, "Kernel version").value.source, "procfs:/proc/sys/kernel/osrelease");
        assert!(spec(c, "System serial number").identifier);
        assert!(spec(c, "System UUID").identifier);
        assert_eq!(spec(c, "System UUID").display(), "Not reported");
    }

    #[test]
    fn root_only_identifiers_report_permission_required() {
        use std::os::unix::fs::PermissionsExt;
        let fx = legion();
        fx.write("sys/class/dmi/id/product_uuid", "abc\n");
        let p = fx.path().join("sys/class/dmi/id/product_uuid");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&p).is_ok() {
            return; // running as root: cannot simulate EACCES
        }
        let c = &collect_from(fx.path())[0];
        assert!(matches!(spec(c, "System UUID").value.availability, Availability::PermissionRequired));
        assert_eq!(spec(c, "System UUID").display(), "Permission required");
        assert_eq!(spec(c, "System UUID").value.source, "sysfs:/sys/class/dmi/id/product_uuid");
    }

    #[test]
    fn legacy_bios_and_chassis_variants() {
        let fx = Fx::new(&[
            ("sys/class/dmi/id/sys_vendor", "Dell Inc.\n"),
            ("sys/class/dmi/id/product_name", "OptiPlex 7010\n"),
            ("sys/class/dmi/id/chassis_type", "3\n"),
            ("sys/firmware/acpi/x", ""),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Chassis").display(), "Desktop");
        assert_eq!(spec(c, "Firmware mode").display(), "BIOS (legacy)");
        assert_eq!(c.title, "OptiPlex 7010");
        assert_eq!(chassis_name(0x80 | 9), Some("Laptop"));
        assert_eq!(chassis_name(31), Some("Convertible"));
        assert_eq!(chassis_name(0), None);
    }

    #[test]
    fn qemu_guest_is_flagged_virtual() {
        let fx = Fx::new(&[
            ("sys/class/dmi/id/sys_vendor", "QEMU\n"),
            ("sys/class/dmi/id/product_name", "Standard PC (Q35 + ICH9, 2009)\n"),
            ("sys/class/dmi/id/chassis_type", "1\n"),
            ("proc/cpuinfo", "processor\t: 0\nflags\t\t: fpu hypervisor\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Virtual machine").display(), "Yes");
        assert_eq!(spec(c, "Hypervisor").display(), "QEMU/KVM");
        assert_eq!(spec(c, "Firmware mode").display(), "Not reported");
    }

    #[test]
    fn hypervisor_flag_without_dmi_hint() {
        let fx = Fx::new(&[("sys/class/dmi/id/sys_vendor", "Acme\n"), ("proc/cpuinfo", "flags : fpu hypervisor\n")]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(spec(c, "Virtual machine").display(), "Yes");
        assert!(spec(c, "Hypervisor").display().contains("not identified"));
        assert_eq!(hypervisor_from_dmi("Microsoft Corporation", "Surface Laptop 5"), None);
        assert_eq!(hypervisor_from_dmi("Microsoft Corporation", "Virtual Machine"), Some("Microsoft Hyper-V"));
        assert_eq!(hypervisor_from_dmi("VMware, Inc.", "VMware7,1"), Some("VMware"));
    }

    #[test]
    fn malformed_and_placeholder_values() {
        let fx = Fx::new(&[
            ("sys/class/dmi/id/sys_vendor", "To Be Filled By O.E.M.\n"),
            ("sys/class/dmi/id/product_name", "To Be Filled By O.E.M.\n"),
            ("sys/class/dmi/id/chassis_type", "banana\n"),
            ("proc/sys/kernel/osrelease", "6.1.0\n"),
        ]);
        let c = &collect_from(fx.path())[0];
        assert_eq!(c.title, "This computer");
        assert!(c.subtitle.is_none());
        assert!(matches!(spec(c, "Chassis").value.availability, Availability::Failed { .. }));
        assert_eq!(spec(c, "Virtual machine").display(), "Not reported");
    }

    #[test]
    fn empty_root_yields_empty() {
        assert!(collect_from(Path::new("/nonexistent/facet")).is_empty());
    }

    /// `cargo test -p facet-collectors -- --ignored --nocapture real_machine_dump`
    #[test]
    #[ignore]
    fn real_machine_dump() {
        for c in collect() {
            println!("== {} [{}] {} / {:?}", c.id, c.category.label(), c.title, c.subtitle);
            for s in c.all_specs() {
                println!("  {:<40} {}  <{}>{}{}", s.label, s.display(), s.value.source, if s.live { " live" } else { "" }, if s.rated { " rated" } else { "" });
            }
        }
    }
}
