# Product and confirmed decisions

## Purpose

Build a beautiful, delightful Rust desktop Device Manager that reveals the physical hardware of the machine on which it runs. Give users an immediate understanding of their machine and a path to detailed specifications, device relationships, and available health information.

## Confirmed requirements

- Written in Rust.
- Linux distribution agnostic, primarily developed and used on Omarchy.
- An independent, distinctive visual identity. Do not support or inherit Omarchy themes.
- Serve both everyday users and power users: approachable summaries with expandable technical detail.
- Version 1 is inspection only.
- Version 2 will allow hardware management; specific operations remain undecided.
- Beauty and delight are central product requirements.

## Primary journeys

1. Know my machine: identify its model, CPU, memory, graphics, storage, and connected hardware.
2. Understand a component: inspect specifications, driver, firmware, connection, and available health readings.
3. Share useful evidence: copy values and export reports for troubleshooting.

## Proposed v1 capabilities

| Area | Details when available |
| --- | --- |
| System | Manufacturer, model, motherboard, chassis, BIOS/UEFI version |
| Processor | Model, architecture, sockets, cores, threads, caches, supported features |
| Memory | Capacity, individual modules, slots, type, speed, manufacturer |
| Graphics | GPU model, driver, bus connection, available VRAM and telemetry |
| Storage | Physical drives, capacity, model, firmware, transport, available health data |
| Network | Physical adapters, driver, capabilities, negotiated link |
| Displays | Manufacturer, model, reported physical size, modes, active connection |
| Peripherals | USB, Bluetooth, input, audio, cameras, hubs, docks |
| Power and sensors | Batteries, charge capacity, cycle count, temperatures, fans, exposed power readings |

Search, device relationships, hotplug feedback, live measurements, and readable text/JSON export are proposed v1 features.

## Data principles

“All physical details” means everything Linux drivers and firmware can reliably expose. Do not invent absent specifications or infer an overall healthy state from incomplete telemetry. SMBIOS values can be missing or inaccurate.

- Distinguish physical devices from partitions, virtual interfaces, and other logical objects.
- Distinguish rated capabilities from current operating values.
- Track source, availability, and freshness per field.
- Distinguish not reported, permission required, disconnected, and collection failed.
- Use precise units; preserve raw evidence where useful for expert inspection.
- Normal browsing should work without administrator access. Restricted read-only details may use explicitly authorized elevated access.

## Boundaries

V1 must not install drivers, flash firmware, control fans, disable devices, or change hardware settings. V2 management commands must be separate from inspection collectors and have their own authorization paths.

## Open decisions

- Final brand name and visual direction; Facet is the proposed concept.
- GUI toolkit: GPUI (user decision, 2026-10-09); accessibility still to be proven in Milestone 1.
- Exact v1 hardware coverage and minimum supported Linux environments.
- V2 management operations.
- Packaging beyond the proposed initial Arch package.
- Performance reference hardware and measured budgets.
