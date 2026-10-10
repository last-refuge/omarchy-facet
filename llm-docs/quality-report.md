# Quality report (v1.0.0)

Reference machine: Lenovo Legion 5 15IRX10, Intel Core i9-14900HX (24 cores / 32 threads), 32 GiB, Intel iGPU
(display) + NVIDIA RTX 5070 Max-Q (kept asleep), Samsung NVMe, Arch/Omarchy, kernel 7.2.5, Hyprland 0.56.2
(Wayland), 2560×1600. Release build. Measured 2026-10-10. Targets come from `implementation-plan.md` and are
proposals, not industry benchmarks. "Unverified" means exactly that: no claim is made.

## Performance

| Target | Result | Evidence |
| --- | --- | --- |
| Usable window within 1 s, slow collectors fill in afterwards | **Met on Wayland.** First paint 197–321 ms (8 runs); every collector finished by 374–554 ms. | `FACET_TIMING=1 ./target/release/facet-app` prints `first-render-ms` and `all-collectors-done-ms` |
| Same on X11 | **Not met.** First paint 1326 ms in the one Xwayland run (X11 backend startup). Single sample; the cause was not investigated. | Xwayland run, `env -u WAYLAND_DISPLAY` |
| Under 1% average idle CPU, live monitoring off | **Met.** 0.0% of one core over 10 s after load. | `/proc/<pid>/stat` ticks |
| Idle CPU with live monitoring on (the default for real hardware) | **About 1.0–1.7% of one core** over 30 s (three runs; noisy because the first battery read after idle takes ~70 ms). Each tick re-reads only the five collectors whose numbers change (processor, memory, power, sensors; graphics every fifth tick); a full read happens on hotplug and Ctrl+R. Updates slow to every 10 s while the window is not focused. The unfocused path is implemented but its CPU was **not measured** (it would need Facet to lose focus, which I did not force on your desktop). | `/proc/<pid>/stat` ticks; `LIVE_COLLECTORS` in `app.rs` |
| Bounded memory | 131 MB RSS, 46 threads idle after load (pipkin on the same GPUI: 140 MB). Sensor history is capped at 90 samples per reading and pruned when a device disappears. | `/proc/<pid>/status`; `HISTORY_CAP` in `app.rs` |
| Not waking the discrete GPU | **Met.** `runtime_status` stayed `suspended` across every launch, CLI run and collector run. Without the guard, the first window takes ~18 s (facet 18.2 s, pipkin 18.4 s) because Vulkan enumeration powers the dGPU up. | `gpu_guard.rs`, `facet --diagnose` |
| All collectors | 148 ms for the full inventory (`facet --diagnose`); slowest single collector 66 ms (power supplies). | `facet --diagnose` |

## Correctness

| Check | Result |
| --- | --- |
| Values cross-checked against trusted tools | **14 of 14 agree** (`scripts/crosscheck.py`): CPU model, cores, threads and max frequency vs `lscpu`; memory and swap bytes vs `free -b`; NVMe capacity and model vs `lsblk`; both GPUs vs `lspci`; USB device count vs `lsusb`; CPU package temperature vs `sensors -j` (±5 °C live drift); battery design capacity vs `upower`. One intentional difference: Facet drops `(R)`/`(TM)` from CPU names. |
| Collector tests | Fixture tests for normal, missing, malformed, restricted and unusual data in every collector. Counts are in the table at the end. |
| Parser robustness | See "Fuzzing" below. |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` is clean. |

## Accessibility and input

| Check | Result |
| --- | --- |
| AT-SPI tree on every screen | **Pass** on 8 screens plus both dialogs (`scripts/a11y-audit.sh`): navigation and main landmarks, headings (page title and each section), every button/link/checkbox named and focusable, search entry, dialogs, list box with options. |
| Keyboard access to all primary workflows | **Pass** (`scripts/keyflow.sh`, real key events with an active-window guard): Tab order sidebar → toolbar → content; Ctrl+K, type, Enter opens a device; Alt+Left back; Ctrl+E export with focus moved into the dialog and trapped there; Esc closes; Enter/Space activates every control; Shift+Tab reverses. |
| Visible focus | Every focusable control has a focus style (accent border/background). Verified by code review and by the focused element reported through AT-SPI; not measured against a separate visual standard. |
| Contrast | **Pass** (unit test `theme::tests::text_pairs_meet_wcag_aa`): 16 text and non-text pairs in light and dark meet WCAG AA (4.5:1 text, 3:1 large text and icons). Two tokens were darkened to pass; see `decisions.md`. |
| Reduced motion | The interface has no animations (the only time-based change is a toast disappearing), so there is nothing to reduce. |
| Screen reader speech | **Unverified.** Orca is installed but speech output was not exercised; only the AT-SPI tree and focus were checked. |

## Platforms and configurations

| Item | Result |
| --- | --- |
| Wayland (Hyprland) | Verified. |
| X11 via Xwayland | Renders and runs correctly (one run, slower start; see Performance). Not tested on a real X11 session. |
| Laptop with dock, hybrid GPU | Verified on real hardware (the reference machine) and with the `laptop` fixture (dock, hubs, two displays). |
| Desktop, virtual machine | **Fixtures only** (`--fixture desktop`, `--fixture vm`): UI and export verified; collectors not run on real desktop or VM hardware. |
| Narrow tiled windows | Verified at 620–640 px (icon rail, stacked rows). Not verified at other widths or on a real tiled layout. |
| Fractional scaling | **Unverified.** |
| Other distributions | **Unverified.** Only Arch/Omarchy was available. The code uses standard sysfs, procfs, kernel uevents and D-Bus properties. |
| True cold boot | **Unverified** (warm runs only). |
| Real hotplug | **Unverified** against a real plug event: the uevent parser and netlink socket are tested, and the full refresh/highlight/removal flow was exercised with `--demo-hotplug`. |
| SATA SMART, AMD GPU telemetry, SMBIOS memory modules | Implemented and fixture-tested; **not run on real hardware** here (no SATA drive, AMD GPU, or readable SMBIOS table). |

## Privacy

Serial numbers, UUIDs and MAC addresses are hidden by default in the interface, CLI and exports (tests in
`facet-core`; verified in the UI after a defect was found: the first live build displayed an NVMe serial in
the clear). Before publishing, the tracked files and the full git history were searched for this machine's
real identifiers; one real NVMe serial in test fixtures was replaced and the local history rewritten.

## Known limitations

- First paint on X11 is about 1.3 s.
- Some firmware details are root-only and shown as "Permission required" (machine serial, UUID, memory modules).
- Reading a suspended dGPU's link speed/width, clocks or temperature would wake it, so they are reported as not queried.
- `libsensors` is not linked: sensors are read from hwmon sysfs, so board-specific label rules in `/etc/sensors3.conf` are not applied.
- No app screenshots are committed (see `decisions.md`).
