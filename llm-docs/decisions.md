# Decisions log

Format: decision, alternatives, why, date. Newest last.

## 2026-10-09 — GUI toolkit: Rust + GPUI (user decision)
- **Decision:** GPUI replaces the earlier GTK4 recommendation.
- **Alternatives:** GTK4 via gtk-rs (previous recommendation).
- **Why:** User choice. `../pipkin` already proves GPUI on this machine.

## 2026-10-09 — GPUI pinned to a Zed git revision, never crates.io 0.2.2
- **Decision:** `gpui` and `gpui_platform` pin Zed rev `a84689073d296dfd39987bc7dd478e43ef76d83a` (same as `../pipkin`), features `wayland`, `x11`, `font-kit`, default features off.
- **Alternatives:** crates.io `gpui` 0.2.2.
- **Why:** 0.2.2 uses the Blade renderer, which enumerates every Vulkan device. On this hybrid Intel + NVIDIA laptop that deadlocked the proprietary NVIDIA driver (processes in uninterruptible D state, reboot required). The pinned revision uses wgpu and follows the compositor's GPU hint (Intel). 0.2.2 also needed a patch for `xattr`/`libc::ENOATTR`.

## 2026-10-09 — Crate names
- **Decision:** `facet-core`, `facet-collectors`, `facet-cli`, `facet-app`. Brand "Facet" remains proposed, not approved.
- **Why:** Cheap to rename later; core and collectors are toolkit-independent.

## 2026-10-09 — Availability model
- **Decision:** every datum is `Field<T>` with `source` and one of available / not_reported / permission_required / disconnected / redacted / failed.
- **Why:** product.md requires distinguishing these states and tracking provenance; a bare `Option` cannot.

## 2026-10-09 — Redaction by default
- **Decision:** serial numbers and MAC addresses are redacted in CLI, UI and exports unless `--include-identifiers` (CLI) is passed.
- **Why:** implementation-plan.md reporting requirement.

## 2026-10-09 — Capacity units
- **Decision:** decimal SI units (1 TB = 10^12 B), matching drive labelling; single shared formatter in `facet-core`.
- **Why:** consistency between UI and CLI; matches vendor and `lsblk -b`-derived expectations.
