# Facet v1.0.0 execution plan

Authoritative instructions for the autonomous run that takes Facet to a releasable v1.0.0. Read this, then `decisions.md`, `field-report.md`, `progress.md`, `implementation-plan.md`, `product.md`, `brand-and-design.md`, `git log` and `git status`. Reference GPUI implementation on this machine: `../pipkin`.

Starting state: Milestone 1 is done and committed. Milestone 2 has started: `crates/core/src/model.rs` (Component, Spec, Inventory) is written and tested but uncommitted. Commit it first.

When a decision is the user's to make, pick the recommended default and record it in `decisions.md` (decision, alternatives, why, date). Do not ask questions.

## Fixed decisions (do not revisit)

- Rust + GPUI pinned to the Zed git revision in the workspace `Cargo.toml`. Never use the crates.io `gpui` 0.2.2: its Blade renderer deadlocked the NVIDIA driver on this laptop.
- v1 is inspection only: no driver installs, firmware flashing, fan control, device disabling, or hardware changes. Read D-Bus properties only; never call methods such as UDisks2 `SmartUpdate` or self-tests.
- Distro-agnostic Linux on Wayland and X11. No Omarchy theme or command dependency. Facet owns its styling (ivory, graphite, vermilion; see `brand-and-design.md`).
- The brand name "Facet" is proposed. Use it, and say in the release notes that it is unconfirmed and trademark-unchecked.
- The GUI runs unprivileged. Any helper must be narrow, read-only, polkit-authorized, with no arbitrary command execution. If a helper is not worth it for v1, ship without it and show "permission required" honestly.
- Serial numbers, UUIDs and MAC addresses are redacted by default in the UI, CLI and exports; including them is opt-in.
- Every datum carries a source and an availability state (available, not reported, permission required, disconnected, redacted, failed). Never invent values or infer "healthy" from incomplete telemetry. Distinguish rated capability from current value, and physical devices from partitions and virtual devices.
- Never wake a sleeping GPU. Keep `gpu_guard`. The GPU collector reads `/sys/bus/pci/devices/<addr>/power/runtime_status` first and reports "suspended, not queried" instead of querying. Waking the dGPU costs about 18 s here.

## Safety rules for this machine

- Launch any GUI or GPU-touching binary with `timeout -s KILL <n>`.
- Before each launch run `ps -eo pid,ppid,stat,comm | awk '$3 ~ /D/ && $2 != 2 && $1 != 2'` (user-space processes only; kernel threads such as `irq/*` and `kworker` blip into D state harmlessly). If stuck D-state processes exist, do non-GPU work only and log it.
- Never run privileged commands, change system or security settings, install packages, or write outside the project and the session scratchpad.
- Build the Arch package with makepkg in the scratchpad only. Never install it.
- No pushing, publishing or sending anything outward. Local git only. Commit at the end of every coherent step. Tag `v1.0.0` locally only when every gate below is met.

## Definition of v1.0.0

### Coverage
System (DMI), Processor, Memory (including modules where readable), Graphics, Storage (including NVMe and SATA health via UDisks2 properties), Network, Displays (including EDID details), Peripherals (PCI, USB, Bluetooth, input, audio, cameras), Power and sensors (batteries, hwmon via libsensors or sysfs). Missing services and unsupported hardware degrade gracefully with an explicit reason.

### Behavior
- Progressive loading: usable window in under 1 s, slower collectors fill in afterward.
- Collection runs off the UI thread.
- Hotplug via udev, with a gentle highlight on new devices.
- Bounded sensor history.
- Refresh preserves selection and scroll position.
- Device removal during collection never crashes, and stale values are never shown as current.
- Connection view: devices behind hubs and docks, drives behind controllers.
- Search (Ctrl+K) over model, manufacturer, identifier and plain-language properties.
- Click-to-copy on values.
- Last-visited page is remembered.

### Reports
Readable text and versioned JSON, with a preview. Identifiers excluded by default. The CLI (`facet`) produces the same report from the same collectors.

### Design
The brand doc's Overview, component pages (summary first, grouped specs, expandable technical detail), loading and error states, wide and narrow tiled windows, light and dark. Critique every screenshot against the brand doc and iterate on typography, spacing, alignment and hierarchy until it is genuinely beautiful. Bundle fonts only if the license permits, and record it.

### Quality gates
Measure each gate and record real numbers in `quality-report.md`. Report anything unmeasured as unverified.
- First render under 1 s (`FACET_TIMING=1`), idle CPU under 1% with live monitoring off, bounded memory.
- Keyboard access to all primary workflows with visible focus.
- Accessibility: the live AT-SPI tree checked with `scripts/a11y-dump.py` for every screen (landmarks, headings, named focusable controls, state). Reduced motion respected. Contrast checked numerically against WCAG AA.
- Fixture tests for missing, malformed, restricted and unusual data. Fuzz or property tests for parsers. `cargo test` and `cargo clippy -- -D warnings` clean.
- Values cross-checked against trusted tools on this machine (lscpu, free, lsblk, lspci, lsusb, sensors, upower, edid-decode if present), with differences explained.
- Verified on this laptop, plus VM, desktop and docked setups via fixture machines. X11 via Xwayland if possible, otherwise listed as unverified.

### Release artifacts
Arch PKGBUILD (built in the scratchpad, never installed), desktop entry, an original app icon matching the brand, README (build, run, privacy, limits), LICENSE with a dependency-license audit including the pinned Zed GPUI crates, `facet --diagnose`, CHANGELOG, version 1.0.0.

## How to work

Keep the milestone order of `implementation-plan.md` (2 design prototype, 3 inventory MVP, 4 complete coverage, 5 live behavior, 6 release). Ship the real collectors early so the UI is built on real data and not only on fixtures.

Per slice:
1. Write fixture tests first for parsers.
2. Implement.
3. Run the CLI on this machine and compare with trusted tools.
4. Screenshot the UI with `grim`, matching the window by PID, and read the image.
5. Check the milestone gate. Fix and re-verify on failure. Do not advance past a failing gate.
6. Update `progress.md`, `decisions.md` and `field-report.md`, then commit.

Use subagents for independent investigation where it saves time (for example one collector per subagent). You own integration and verification.

## Done when

Every v1.0.0 item above is met or explicitly marked unverified with a reason. Then:
1. Write `llm-docs/release-readiness.md`: what shipped, measured numbers, unverified items, known limitations, next steps for v2.
2. Tag `v1.0.0` locally.
3. Stop.

Stop earlier only if a safety rule would be violated by continuing, and say which.
