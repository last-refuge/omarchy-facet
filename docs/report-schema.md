# Report schema (version 1)

`facet --json` and the app's Export dialog produce this document. It is versioned: consumers should
check `schema_version` and ignore unknown fields.

```jsonc
{
  "schema_version": 1,
  "generator": "facet 1.0.0",
  "inventory": {
    "components": [
      {
        "id": "storage:nvme0n1",          // stable across refreshes
        "category": "storage",            // system | processor | memory | graphics | storage | network | displays | peripherals | power
        "title": "SAMSUNG MZAL81T0HFLB-00BL2",
        "subtitle": "1.0 TB · NVMe solid-state drive",   // optional
        "parent": null,                   // id of the component this is connected through, or null
        "summary": [ /* Spec */ ],        // the facts shown first
        "groups": [ { "title": "Identity", "technical": false, "specs": [ /* Spec */ ] } ]
      }
    ]
  }
}
```

## Spec

```jsonc
{
  "label": "Capacity",
  "value": {
    "state": "available",                 // see below
    "value": { "unit": "bytes", "v": 1024209543168 },
    "source": "sysfs:/sys/block/nvme0n1/size"   // where the value came from
  },
  "identifier": false,                    // serial number, UUID, MAC address: hidden unless included
  "live": false,                          // a measurement that changes (temperature, usage)
  "rated": false                          // a rated capability rather than a current operating value
}
```

### `state`

| state | meaning |
| --- | --- |
| `available` | `value` is present |
| `not_reported` | the device or driver does not expose it |
| `permission_required` | it exists but reading it needs administrator access |
| `disconnected` | the device went away |
| `redacted` | present but withheld from this report (identifiers by default) |
| `failed` | reading failed; `reason` says why |

Facet never invents a value: absent data keeps its state and `source`.

### `unit` (for `available` values)

`text`, `bytes` (decimal, drive labels), `binary_bytes` (KiB/MiB/GiB), `hertz`, `celsius` (always degrees Celsius in JSON, whatever unit the interface shows), `percent`,
`count`, `bool`, `watts`, `volts`, `rpm`, `bps` (bits per second).

## Privacy

Serial numbers, UUIDs and MAC addresses are `redacted` unless `--include-identifiers` is passed (or the
checkbox is ticked in the Export dialog). Hostnames and user names are never collected.
