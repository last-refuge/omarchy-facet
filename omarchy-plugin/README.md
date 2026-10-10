# Facet for the Omarchy bar

A small bar widget for [Omarchy](https://omarchy.org)'s shell: your CPU temperature (or another live
reading) in the bar, and a popup with the headline readings and a way into
[Facet](https://github.com/last-refuge/omarchy-facet), the hardware explorer.

- **Click** the icon: popup with CPU temperature, memory in use, GPU temperature, battery and drive temperature.
- **Right-click**: open Facet. **Middle-click**: refresh now.
- It never wakes a sleeping discrete GPU: it shows "asleep, not queried" instead of a number.
- It only reads. It does not write files, use the network, or change any hardware setting.

## Install

The Facet installer offers to add this for you on Omarchy:

```bash
curl -fsSL https://raw.githubusercontent.com/last-refuge/omarchy-facet/master/install.sh | bash
```

Or add it yourself (Facet must be installed; the widget shows an install hint if it is not):

```bash
omarchy plugin add https://github.com/last-refuge/omarchy-facet-plugin.git --enable
```

Or by hand, which is what the installer does:

```bash
cp -r /usr/share/facet/omarchy-plugin ~/.config/omarchy/plugins/facet.hardware
omarchy-shell shell rescanPlugins
omarchy plugin enable facet.hardware
```

Plugins run unsandboxed inside `omarchy-shell`; this one is two small files (`manifest.json`, `Widget.qml`)
that you can read in a minute.

## Settings

Edit the widget in your bar layout (`omarchy bar`), or `~/.config/omarchy/shell.json`:

| Setting | Default | |
| --- | --- | --- |
| `pillReading` | `cpu_temperature` | `cpu_temperature`, `memory_in_use`, `gpu_temperature`, `battery`, `storage_temperature`, or `none` for just the icon |
| `refreshIntervalSec` | `15` | How often the bar value updates (5–300). The popup refreshes every 3 s while open. |

Move it with `omarchy bar move facet.hardware <left|center|right>`. A keybinding can toggle the popup with
`omarchy-shell facet.hardware toggle`.

## Remove

```bash
omarchy plugin remove facet.hardware
```

## How it works

Every refresh runs `facet --status`, which prints a few readings as JSON in about a tenth of a second. The
widget is a standard Omarchy `bar-widget` plugin (`manifest.json` plus `Widget.qml`).
