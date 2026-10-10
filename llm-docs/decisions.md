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

## 2026-10-09 — Never wake a sleeping GPU (gpu_guard)
- **Decision:** at startup, if a GPU of a different vendor than the boot GPU is runtime-suspended, set `VK_LOADER_DRIVERS_SELECT` to the boot GPU's driver globs, unless the user already set any `VK_*` driver variable. GPU collectors must read `runtime_status` before querying a device and report "suspended, not queried".
- **Alternatives:** hardcoded Intel ICD path (distro-specific); leave the loader alone (18 s startup, changes hardware power state); ask users to set env vars.
- **Why:** measured 18 s to first window with the dGPU waking vs 170–235 ms without. An inspector should not alter the state it reports.

## 2026-10-09 — v1 never calls UDisks2 methods
- **Decision:** read UDisks2 properties only. No `SmartUpdate`, self-test, or other methods in v1.
- **Why:** product boundary (inspection only); those calls need authorization and have side effects.

## 2026-10-10 — Push to remote at the end of the run (user instruction)
- **Decision:** at the end of the v1.0.0 run, commit and push the branch to `https://github.com/last-refuge/omarchy-facet.git` (the only remote authorised). Tags stay local unless the user asks.
- **Why:** explicit user instruction mid-run; it overrides the earlier "no pushing" rule for this one destination only.
- **Pre-push audit (required):** grep the tree for real identifiers captured during development (drive serials, MAC/Bluetooth addresses, UUIDs, hostnames, usernames) and replace them with obviously fictional values before pushing.

## 2026-10-10 — Vermilion deepened to #D1452D (light) for AA button text
- **Decision:** light accent fill is `#D1452D` (brand concept `#D84B33`), dark `on_accent` is `#101312`; light "faint" text is `#636864`.
- **Alternatives:** keep `#D84B33` and use large/bold-only text on it; dark text on vermilion.
- **Why:** numeric WCAG check (unit test `theme::tests::text_pairs_meet_wcag_aa`): white on `#D84B33` is 4.22:1, below AA for 13 px semibold button labels; `#D1452D` gives 4.57:1 and still reads as the brand vermilion. Faint text on ivory was 4.23:1; now 5.04:1.

## 2026-10-10 — No app screenshots committed
- **Decision:** the repo contains the generated concept art (`design/concepts/`) but no screenshots of the running app.
- **Alternatives:** force the window opaque with a Hyprland rule/`hyprctl eval`; fullscreen captures; crop bleed.
- **Why:** Omarchy applies a default 0.985/0.96 window opacity, so captures of the real window show faint text from whatever is behind it (observed: other applications' content). GPUI on Linux cannot render offscreen (`render_to_image` is macOS/Windows/test only), and overriding the rule means changing the user's compositor configuration. `scripts/shot.sh` is for local design review only; its output must not be committed.

## 2026-10-10 — Licence MIT OR Apache-2.0; third-party audit
- **Decision:** Facet is `MIT OR Apache-2.0`. `scripts/license-audit.py` walks the shipped dependency graph (Linux) and fails on anything outside a permissive allow-list; `THIRD-PARTY-LICENSES.md` is generated from it.
- **Result:** no GPL/AGPL crates; the 23 git-sourced crates (Zed's GPUI revision and forks) are Apache-2.0 or MIT; one unusual permissive licence (`bzip2-1.0.6`) allow-listed. Bundled assets: IBM Plex Sans and Lilex (SIL OFL 1.1), Lucide icons (ISC).

## 2026-10-10 — Live monitoring on by default (user decision)
- **Decision:** for real hardware, live monitoring starts on (re-read every 2 s, three minutes of history); demo fixtures stay still unless `--live`. The choice is remembered (`live` in `state.json`) and `--live` / `--no-live` override it for a run.
- **Alternatives:** off by default (the earlier choice, to meet the <1% idle target); a longer interval.
- **Why:** the overview's trend lines and "Refreshing every 2 seconds" are central to the concept design, and a hardware monitor that is frozen by default reads as broken.
- **Cost, and how it is contained:** about 1.0–1.7% of one core while focused (0.0% when turned off). A tick re-reads only processor, memory, power and sensors (graphics every 10 s); full reads happen on hotplug and Ctrl+R; a live tick never supersedes an in-flight full refresh; updates drop to every 10 s while the window is unfocused. The plan's <1% target stays defined "with live monitoring off".

## 2026-10-10 — Temperature unit: Fahrenheit by default, one shared setting (user decision)
- **Decision:** a Celsius/Fahrenheit preference, default Fahrenheit. Stored in `$XDG_CONFIG_HOME/facet/settings.json` and read by the app, the `facet` CLI and therefore the bar widget (which shows the CLI's ready-formatted `display`). `--celsius` / `--fahrenheit` and `FACET_TEMPERATURE_UNIT` override it for one run. Values are stored, charted and exported in Celsius; only display converts.
- **Alternatives:** a locale-based default; per-surface settings; converting at collection time.
- **Why:** one file means every surface agrees; keeping data in Celsius keeps JSON unambiguous and history/charts unit-free. The unit is a process-wide display setting in `facet-core` (set once at startup, or on a Preferences change) so no formatting call site needed changing; the library default stays Celsius so tests and library users are deterministic, and the apps apply the user's default (Fahrenheit).
- **Limits:** the unit is global to the process, so a library consumer changing it affects all its threads. A `FACET_TEMPERATURE_UNIT` override is never written to the file.
