# Architecture and delivery plan

This is the proposed implementation plan, updated to reflect distribution independence and the rejection of Omarchy theme integration. No implementation has started.

## Status (2026-10-10)

Milestones 1–6 are implemented; measured results, gates and what remains unverified are in
[quality-report.md](quality-report.md). Decisions made along the way are in [decisions.md](decisions.md),
the build log is [progress.md](progress.md), and the execution spec for v1.0.0 is [v1-plan.md](v1-plan.md).

## Foundation

Use Rust with GPUI (user decision; see [decisions.md](decisions.md)), pinned to a Zed git revision, subject to a prototype proving accessibility, rendering, and performance. GTK4 was the earlier recommendation and is superseded. Target Wayland and X11. Use standard Linux interfaces and runtime capability detection, without dependencies on Omarchy commands or configuration.

GPUI is pre-1.0 and pinned to a Zed git revision; the crates.io release must not be used (see decisions.md).

## Cargo workspace responsibilities

- Core: typed hardware records, relationships, units, availability states, report schema.
- Collectors: Linux inventory, sensors, device events, optional privileged read-only queries.
- Application: GPUI views, navigation, search, presentation state.
- CLI: diagnostic inventory and JSON export using the same collectors.

Collect hardware data outside the UI thread. Load progressively, cache static specifications, preserve selection during refresh, and refresh live measurements only as needed. Bound retained sensor history. Handle device removal during collection without crashing or showing stale values as current.

## Collection approach

Use Linux device APIs and sysfs for inventory, D-Bus services where appropriate, and structured command output only when necessary. Evaluate the exact libraries and service dependencies in the feasibility milestone.

Prefer libsensors for sensor interpretation because raw readings may require board-specific labeling and conversion. Use available storage services or narrowly scoped probes for health data; capability and permission availability must be explicit.

Keep the GUI unprivileged. If restricted information requires a helper, constrain it to specific read-only operations and authorize through polkit. Do not expose arbitrary command execution.

Reports should support readable text and versioned JSON, a preview, and exclusion of serial numbers and MAC addresses by default.

## Milestones

| Milestone | Deliverable | Completion gate |
| --- | --- | --- |
| 1. Feasibility | GPUI prototype and collector spike | Runs on Omarchy; records accessible, restricted, and unavailable fields; proves independent styling |
| 2. Design prototype | Overview, component detail, search, loading and error states | Core journeys work with realistic fixtures in wide and narrow windows |
| 3. Inventory MVP | System, CPU, memory, GPU, storage, PCI and USB | Values agree with trusted tools on representative machines |
| 4. Complete coverage | Displays, network, audio, Bluetooth, batteries, sensors | Missing services and unsupported hardware degrade gracefully |
| 5. Live behavior | Hotplug, bounded history, refresh, reconnection | No UI freezes, duplicate devices, or stale readings presented as current |
| 6. Release | Reports, packaging, accessibility, performance polish | Installable Arch package, desktop entry, icon, documentation, diagnostics |

Start with one complete vertical slice: discover a real storage device, show its summary and technical details, copy a value, and export it.

The initial Arch package serves the primary Omarchy environment. Keep installation and dependencies portable so other distributions can be supported without application redesign.

## Proposed quality targets

Measure on an agreed reference machine; these are targets, not existing results.

- Usable window within one second, with slower collectors populating afterward.
- Under 1% average idle CPU with live monitoring disabled.
- Keyboard access to all primary workflows.
- No crashes if devices disappear during collection.
- Parser and fixture tests covering missing, malformed, restricted, and unusual data.
- Verification on laptop, desktop, docked setup, and virtual machine.
- Verify Wayland and X11, narrow tiled windows, fractional scaling, visible focus, screen readers, and reduced motion.

## Technical references consulted

- Linux device model: https://cdn.kernel.org/doc/html/latest/driver-api/driver-model/overview.html
- Sensor interpretation and sysfs: https://www.kernel.org/doc/html/latest/hwmon/sysfs-interface.html
- SMBIOS reliability: https://www.nongnu.org/dmidecode/
- Systemd device enumeration: https://www.freedesktop.org/software/systemd/man/latest/sd-device.html
- UDisks ATA health interface: https://storaged.org/udisks/docs/gdbus-org.freedesktop.UDisks2.Drive.Ata.html

Omarchy themes were researched in the original plan, but theme integration was explicitly rejected and is not part of this architecture.
