# Changelog

## 1.0.0

First release. Facet is an inspection-only hardware explorer for Linux, written in Rust with GPUI.

- Browse the machine's system, processor, memory, graphics, storage, network, displays, peripherals, and power & sensors, from a one-screen overview down to technical details.
- Every value records where it came from and whether it is available, not reported, needs administrator access, or failed. Nothing is invented.
- Reads storage health (NVMe and SATA) from UDisks2 properties, EDID panel details, SMBIOS memory modules when readable, batteries, and hwmon sensors.
- Never wakes a sleeping discrete GPU: it is reported as suspended instead of being queried.
- Search everything with Ctrl+K; click any value to copy it; follow the connection view from docks and hubs to the devices behind them.
- Hotplug detection through kernel uevents; optional live monitoring with short trend charts.
- Readable text and versioned JSON reports with a preview. Serial numbers, UUIDs and MAC addresses are hidden by default in the interface, the CLI and exports.
- `facet` command-line tool with `--json`, `--include-identifiers` and `--diagnose`.
- Omarchy bar widget (`omarchy-plugin/`): live readings in the bar and a popup, installed with the one-command setup. `facet --status` prints the headline readings as JSON.
- One-command installer (`install.sh`) for Arch and Omarchy, and a no-root source install for other distributions.
- Light and dark themes, keyboard access to every control, accessible names and landmarks (AT-SPI), Wayland and X11.
