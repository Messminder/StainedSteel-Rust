# StainedSteel Rust (KISS Edition)

Simple, purpose-built Rust driver for SteelSeries Apex 5 OLED on Linux inspired by Steelclock's Go drivers. It's stained steel because it's written in Rust.

This driver has been vibecoded for my own computer from SteelClock's sources because i am a menace. Install at your own risks and perils.
Works on an Apex 5 keyboard. Other keyboards untested.

## Scope

- Loads a single JSON profile: `profiles/dashboard.json`
- Renders a dashboard with: CPU, volume, keyboard lock states, memory graph, network speeds
- Sends frames directly to Apex 5 hidraw interface (`VID 1038`, `PID 161C`, `mi_01`)
- No plugin system, no web editor, no profile manager, respected the KISS principle as much as an AI could.

## Run

```bash
cargo run --release
```

Optional config path:

```bash
cargo run --release -- --config profiles/dashboard.json
```

Single frame then exit:

```bash
cargo run --release -- --one
```

Single frame with explicit config:

```bash
cargo run --release -- --config profiles/dashboard.json --one
```

## Permissions

You need write access to `/dev/hidraw*` for the keyboard display interface.

Use the existing udev rule from the Go project if needed:

- `Go/profiles/99-steelseries.rules`

## Notes

- Frame format matches the Go Linux direct driver:
  - Packet size: `642`
  - Packet bytes: `0x61 + 640 bytes frame + padding`
  - Frame bytes: `128x40 mono`, row-major, MSB-first
- Volume reads from `amixer get Master`.
