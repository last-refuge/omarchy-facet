# Field report: what this machine exposes

Reference machine: Lenovo Legion 5 15IRX10 (83LY), Intel Raptor Lake-S iGPU + NVIDIA RTX 5070 Max-Q (hybrid), Samsung NVMe, Arch/Omarchy, kernel 7.2.5, Hyprland/Wayland. Measured 2026-10-09. Unprivileged user, no polkit prompts. Update as milestones add coverage.

Legend: **A** accessible unprivileged · **R** restricted (root/polkit) · **U** unavailable / not exposed.

## System (DMI)
| Field | Source | State |
| --- | --- | --- |
| Manufacturer, model, version | `/sys/class/dmi/id/{sys_vendor,product_name,product_version}` | A |
| Board vendor/name | `/sys/class/dmi/id/board_*` | A |
| BIOS version, date | `/sys/class/dmi/id/bios_*` | A |
| Chassis type | `/sys/class/dmi/id/chassis_type` (10 = notebook) | A |
| Product serial, board serial, UUID | `/sys/class/dmi/id/*serial`, `product_uuid` | **R** (file exists, mode denies read) |
| Memory modules (SMBIOS type 17) | `dmidecode -t memory` | **R** (`/sys/firmware/dmi/tables/*` unreadable) |
| Memory module detail via SPD | hwmon `spd5118` exposes temperatures only | partial |

## Storage (NVMe)
| Field | Source | State |
| --- | --- | --- |
| Model, firmware, capacity, transport, rotational, removable | sysfs `/sys/block/*`, `/sys/class/nvme` | A |
| Serial | sysfs `device/serial` | A, **redacted by default** |
| Health: critical warning, power-on hours, temperature, self-test status | UDisks2 `NVMe.Controller` properties (D-Bus system bus) | A (cached values; `SmartUpdated` timestamp gives freshness) |
| Temperature (live) | hwmon `nvme` `temp*_input` | A |
| Refreshing SMART (`SmartUpdate`), self-tests | UDisks2 methods | not used: v1 is inspection only; self-test is management |
| SATA SMART attributes | `smartctl` not installed | U here; plan for UDisks2 ATA interface |

Note: UDisks `SmartTemperature` (K, cached) and hwmon (°C, live) differ by about 1 °C because of sampling time. Show the source and freshness, never merge them silently.

## Sensors
`/sys/class/hwmon` exposes ADP0, BAT0, nvme, lenovo_wmi_other, spd5118 ×2, coretemp, ucsi power supplies, mt7925 (Wi-Fi). Interpretation via libsensors is a Milestone 4 task; the library exists but has no pkg-config file (link manually).

## Graphics: do not wake the dGPU
Enumerating Vulkan adapters powers the suspended NVIDIA GPU out of runtime PM, taking ~18 s here. An inspector must not change the power state of what it inspects. Reading the dGPU's telemetry (NVML, `nvidia-smi`, hwmon, D-Bus) can wake it too: the GPU collector must check `/sys/bus/pci/devices/<addr>/power/runtime_status` first and report "suspended, not queried" rather than waking it.

## GPUI feasibility results
| Gate | Result |
| --- | --- |
| Window opens on Wayland (Hyprland), Intel iGPU | Pass, pinned Zed rev (wgpu). crates.io 0.2.2 (Blade) fails on this hardware: see decisions.md. |
| Time to first render | 170–235 ms over 5 runs, dGPU left suspended (with `gpu_guard`). 18.2 s without it (facet) and 18.4 s (pipkin, same GPUI) when Vulkan wakes the dGPU. |
| Independent styling | Pass: ivory/graphite/vermilion drawn by the app; no GTK/libadwaita theme inheritance involved. |
| Accessibility tree (AT-SPI) | Pass for structure: live tree shows application → frame → landmark "Storage overview" → headings → focusable buttons with names and a `click` action. Verified with `scripts/a11y-dump.py`. Screen-reader speech not yet verified with Orca. |
| Keyboard focus | Rows are `tab_index(0)` with a visible focus ring and Enter/Space activation (visual verification pending). |
| Click to copy | Implemented; clipboard content not yet verified programmatically. |

## Open risks carried forward
1. Orca speech output and focus announcements untested.
2. Fractional scaling and narrow/tiled windows untested.
3. X11 backend untested (feature enabled).
4. Zed git revision is pre-1.0; packaging must vendor or pin it.
