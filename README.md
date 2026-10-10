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

On Arch Linux and derivatives (including Omarchy):

```bash
git clone https://github.com/last-refuge/omarchy-facet.git
cd omarchy-facet
scripts/build-package.sh        # builds a package in ./target/package, never installs
sudo pacman -U target/package/facet-*.pkg.tar.zst
```

Facet is distribution-independent; the package is just the first one provided. To build from source anywhere:

```bash
# needs: Rust (see rust-toolchain.toml), clang, cmake, git, pkg-config,
#        wayland, libxkbcommon(-x11), libxcb, fontconfig, a Vulkan driver
cargo build --release
./target/release/facet-app      # the app
./target/release/facet          # the command-line tool
```

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
facet --diagnose              # what Facet can and cannot read here; collector timings
```

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
```

Run the tests with `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.
`scripts/` has the accessibility audit, keyboard-flow test and cross-check against `lscpu`, `free`,
`lsblk`, `lspci`, `lsusb`, `sensors` and `upower`.

## Status and limits

Version 1.0 is inspection only. Verified on one Arch/Omarchy laptop (Wayland, Intel + NVIDIA hybrid) and
with fixture machines for a desktop and a virtual machine. See `llm-docs/quality-report.md` for what was
measured and what was not. The name "Facet" is provisional and has not been checked for trademarks.

## Licence

MIT or Apache-2.0, at your option. Third-party licences are listed in `THIRD-PARTY-LICENSES.md`.
Bundled fonts (IBM Plex Sans, Lilex) are under the SIL Open Font License; icons are Lucide (ISC).
