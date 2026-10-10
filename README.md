# Facet

**Know your machine.** Facet is a hardware explorer for Linux: a calm, detailed look at every component
in your computer, from "what CPU is this?" down to firmware versions, link speeds and drive health.
It only ever reads. It never changes your hardware.

<img src="design/concepts/facet-overview.png" alt="Concept art for Facet's overview" width="720">

*Concept art, not a screenshot.*

## What you get

- **An overview** of the machine, then pages for **System, Processor, Memory, Graphics, Storage, Network,
  Displays, Peripherals, and Power & sensors**. Each component leads with a readable summary and keeps the
  technical detail one click away.
- **Honest data.** Every value shows where it came from (turn on *Show sources*) and why it might be
  missing: not reported by the device, needs administrator access, or failed. Facet never invents a
  value and never calls a drive "healthy" from incomplete data.
- **Connections.** See which devices hang off which hub or dock, which display is on which GPU.
- **Search everything** with <kbd>Ctrl</kbd>+<kbd>K</kbd>. Click any value to copy it.
- **Reports** as readable text or versioned JSON, with a preview, for bug reports and support.
- **Live updates.** Plug something in and it appears. Optional live monitoring draws short trend charts.
- **Light and dark themes**, full keyboard access, screen-reader landmarks and names.

## Privacy

Facet reads hardware information on your computer and sends nothing anywhere. Serial numbers, UUIDs and
MAC addresses are **hidden by default** in the interface, the command line and exported reports until you
choose to show or include them. Hostnames and user names are never collected.

## Install

**Arch Linux and Omarchy.** One command:

```bash
curl -fsSL https://raw.githubusercontent.com/last-refuge/omarchy-facet/master/install.sh | bash
```

It installs a real package (so `pacman` can update and remove it), checks it works, and tells you how to
open it. On Omarchy, press <kbd>Super</kbd>+<kbd>Space</kbd> and type "Facet". It downloads a prebuilt,
checksum-verified package when a release exists and otherwise builds from source in a few minutes. It asks
for your password only for the final `pacman -U`.

Prefer to read it first? `install.sh` is short. Options: `--dry-run` shows the plan and changes nothing,
`--source` forces a source build, `--prefix DIR` installs without administrator access, `--bar` / `--no-bar`
adds or skips the Omarchy bar widget (by default Omarchy users are asked), `--uninstall` removes it, and
`--yes` skips confirmations (it never edits your shell config on its own).

**Any other Linux.** The same command builds from source into `~/.local` and needs no administrator
access. It tells you exactly which development packages to install if any are missing. Or by hand:

```bash
# needs: Rust (https://rustup.rs), git, clang, cmake, pkg-config, wayland, libxkbcommon(-x11), libxcb,
#        fontconfig, freetype, and a Vulkan driver
make install PREFIX=~/.local
```

**Remove it:** `sudo pacman -Rns facet` (Arch) or `./install.sh --uninstall`.

## Use

| Key | Action |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> | Search all hardware |
| <kbd>Ctrl</kbd>+<kbd>E</kbd> | Export a report |
| <kbd>Ctrl</kbd>+<kbd>R</kbd> | Re-read the hardware |
| <kbd>Alt</kbd>+<kbd>←</kbd> | Back |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> | Show or hide data sources |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>I</kbd> | Show or hide serial numbers and addresses |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>M</kbd> | Live monitoring on/off |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> | Light / dark |

```bash
facet                         # readable report
facet --json                  # versioned JSON (see docs/report-schema.md)
facet --include-identifiers   # include serials, UUIDs and MAC addresses
facet --status                # a few live readings as JSON (about 35 ms; used by the bar widget)
facet --diagnose              # what Facet can and cannot read here; collector timings
```

## Omarchy bar widget

On Omarchy the installer offers to add a small widget to your bar: live CPU temperature, and a popup with
memory, GPU, battery and drive readings plus a button that opens Facet. See
[`omarchy-plugin/`](omarchy-plugin/README.md). It never wakes a sleeping GPU, and it only reads.

## What needs administrator access

Normal browsing never does. Some firmware details are root-only on most systems and show as
"Permission required": the machine's serial number and UUID, and the individual memory modules (SMBIOS).
Facet does not ask for elevation; run `facet --diagnose` to see what is readable on your machine.

## Hybrid graphics

Looking at a sleeping discrete GPU can power it up, which takes seconds and changes the state of the
hardware being inspected. Facet reads the discrete GPU's identity from cached data only and reports it as
*suspended, not queried*. On hybrid laptops it also tells the Vulkan loader to use the display GPU's
driver, so starting Facet does not wake the other one. If you set `VK_DRIVER_FILES`, `VK_ICD_FILENAMES` or
`VK_LOADER_DRIVERS_SELECT` yourself, Facet leaves them alone.

## Built with

Rust and [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), pinned to one revision (the
crates.io release uses an older renderer that can hang hybrid-GPU laptops).

```
crates/core        hardware records, availability states, units, report schema
crates/collectors  Linux collectors (sysfs, procfs, D-Bus properties, uevents); pure and fixture-tested
crates/cli         the `facet` command
crates/app         the GPUI application
omarchy-plugin/    the Omarchy bar widget (manifest.json + Widget.qml)
install.sh         the one-command installer; packaging/ has the PKGBUILD and desktop entry
```

Run the tests with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
`scripts/` has the accessibility audit, keyboard-flow test, plugin contract test (`plugin-test.sh`) and cross-check against `lscpu`, `free`,
`lsblk`, `lspci`, `lsusb`, `sensors` and `upower`.

## Status and limits

Version 1.0 is inspection only. Verified on one Arch/Omarchy laptop (Wayland, Intel + NVIDIA hybrid) and
with fixture machines for a desktop and a virtual machine. See `llm-docs/quality-report.md` for what was
measured and what was not. The name "Facet" is provisional and has not been checked for trademarks.

## Licence

MIT or Apache-2.0, at your option. Third-party licences are listed in `THIRD-PARTY-LICENSES.md`.
Bundled fonts (IBM Plex Sans, Lilex) are under the SIL Open Font License; icons are Lucide (ISC).
