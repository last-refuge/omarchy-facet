//! Avoid waking a sleeping discrete GPU just to draw our window.
//!
//! On hybrid laptops the Vulkan loader enumerates every installed driver, which powers the dGPU up
//! out of runtime suspend (measured here: ~18 s to first window, vs ~0.2 s when avoided). A hardware
//! inspector must not change the power state of the hardware it inspects, so when another GPU is
//! runtime-suspended we ask the loader to use only the boot GPU's driver. Explicit `VK_*` settings
//! from the user always win.

use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuInfo {
    pub vendor: u32,
    pub boot_vga: bool,
    pub runtime_suspended: bool,
}

const USER_OVERRIDES: [&str; 3] = ["VK_DRIVER_FILES", "VK_ICD_FILENAMES", "VK_LOADER_DRIVERS_SELECT"];

/// Loader glob(s) matching the Vulkan driver manifests for a PCI vendor id.
fn driver_globs(vendor: u32) -> Option<&'static str> {
    match vendor {
        0x8086 => Some("intel*"),
        0x1002 => Some("radeon*,amd*"),
        0x10de => Some("nvidia*"),
        _ => None,
    }
}

/// Pure decision: `Some(globs)` only when a boot GPU exists and a different-vendor GPU is suspended.
pub fn decide(gpus: &[GpuInfo]) -> Option<&'static str> {
    let boot = gpus.iter().find(|g| g.boot_vga)?;
    let other_sleeping = gpus.iter().any(|g| !g.boot_vga && g.runtime_suspended && g.vendor != boot.vendor);
    if other_sleeping { driver_globs(boot.vendor) } else { None }
}

fn read_hex(path: &Path) -> Option<u32> {
    let s = fs::read_to_string(path).ok()?;
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

pub fn scan(root: &Path) -> Vec<GpuInfo> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let d = e.path();
            // PCI class 0x03xxxx = display controller.
            if read_hex(&d.join("class"))? >> 16 != 0x03 {
                return None;
            }
            Some(GpuInfo {
                vendor: read_hex(&d.join("vendor"))?,
                boot_vga: fs::read_to_string(d.join("boot_vga")).is_ok_and(|s| s.trim() == "1"),
                runtime_suspended: fs::read_to_string(d.join("power/runtime_status")).is_ok_and(|s| s.trim() == "suspended"),
            })
        })
        .collect()
}

/// Call first thing in `main`, before any other thread exists and before GPUI initialises Vulkan.
pub fn apply() {
    if USER_OVERRIDES.iter().any(|k| std::env::var_os(k).is_some()) {
        return;
    }
    if let Some(globs) = decide(&scan(Path::new("/sys/bus/pci/devices"))) {
        // SAFETY: single-threaded at this point in main; nothing else reads the environment yet.
        unsafe { std::env::set_var("VK_LOADER_DRIVERS_SELECT", globs) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vendor: u32, boot_vga: bool, runtime_suspended: bool) -> GpuInfo {
        GpuInfo { vendor, boot_vga, runtime_suspended }
    }

    #[test]
    fn restricts_when_other_vendor_gpu_is_asleep() {
        assert_eq!(decide(&[gpu(0x8086, true, false), gpu(0x10de, false, true)]), Some("intel*"));
    }

    #[test]
    fn leaves_loader_alone_when_dgpu_is_awake_or_absent() {
        assert_eq!(decide(&[gpu(0x8086, true, false), gpu(0x10de, false, false)]), None);
        assert_eq!(decide(&[gpu(0x8086, true, false)]), None);
        assert_eq!(decide(&[]), None);
    }

    #[test]
    fn unknown_boot_vendor_is_not_restricted() {
        assert_eq!(decide(&[gpu(0x1234, true, false), gpu(0x10de, false, true)]), None);
    }
}
