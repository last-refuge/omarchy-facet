# Progress log

Autonomous build log. Newest last. Each entry: what was done, what was measured, what remains.

## 2026-10-09 — Start
- Workspace: core, collectors, cli, app (GPUI, pinned Zed rev).
- Storage vertical slice works in CLI and GPUI window (verified by screenshot on Hyprland/Wayland, Intel iGPU).
- Open: accessibility spike, startup time measurement, field report.

## 2026-10-09 — Milestone 1 (Feasibility): complete
- Measured: first render 170–235 ms (5 runs, dGPU suspended and untouched); 18.2 s without gpu_guard. Pipkin on the same GPUI takes 18.4 s on this laptop for the same reason.
- AT-SPI tree verified by script (landmark, headings, focusable named buttons with click action).
- Field report written: llm-docs/field-report.md. Decisions logged.
- Unverified: Orca speech, fractional scaling, X11, clipboard contents.
- Next: Milestone 2 (design prototype: Overview, detail, search, loading/error states, wide/narrow).

## 2026-10-10 — Milestones 2–4 largely landed
- Four collector agents (system/cpu/memory, graphics/displays, network/peripherals, power/sensors/storage-health) merged; 114 collector tests, clippy clean.
- All collectors finish in ~0.10 s on the reference laptop with the dGPU left suspended.
- UI: sidebar/toolbar, Overview, category lists, component detail, connections, preferences, Ctrl+K palette, export dialog, loading/error states, light/dark, narrow rail. Reviewed by screenshot against fixtures and live data.
- Found and fixed from live review: false "new device" highlights during first load, unlabeled facts, wrong GPU "asleep" note, noisy Peripherals (46 -> curated), serial numbers shown in the UI (now hidden by default with a toggle).
- Finding: reading PCIe `current_link_speed/width` wakes a suspended dGPU; those are left unread when suspended (see field-report.md).
- Open: hotplug (udev), live monitoring + bounded history, quality measurements, X11 check, fuzz tests, packaging/release artifacts, privacy audit before push.
