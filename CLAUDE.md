# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A Cargo workspace for a Raspberry Pi Pico W (RP2040) fan controller that drives two ebm-papst
RadiCal centrifugal fans over Modbus RTU and integrates with Home Assistant over MQTT.
The firmware is `no_std` and built on Embassy. The other crates are supporting libraries and
desktop-side helper binaries.

## Commands

**Always run cargo from the package directory, never the workspace root.** `fan-controller` only
compiles for `thumbv6m-none-eabi`, which is set in `fan-controller/.cargo/config.toml`; cargo only
reads that file when invoked from inside that directory (see [README.md](README.md)).

```bash
cd fan-controller && cargo run
```

`cargo run` in `fan-controller` flashes and runs on a connected RP2040 via `probe-rs run --chip
RP2040` (the configured runner) — it requires a debug probe. To flash without a probe, swap the
runner in `.cargo/config.toml` to `elf2uf2-rs -d`. `DEFMT_LOG=debug` is set there too, so RTT logs
come out at debug level.

Tests, checks, and lints run per crate from that crate's directory:

```bash
cd home_assistant_discovery && cargo test
```

```bash
cd home_assistant_discovery && cargo test -- serialize_custom_example
```

`home_assistant_discovery` uses `insta` snapshots (`src/snapshots/`); accept changes with
`cargo insta review`. `fan-controller` has a `bacon.toml`; `bacon` defaults to `cargo check`, `c`
is bound to `clippy-all`, and `bacon test -- <name>` runs a single test.

## Build-time configuration (fan-controller)

`fan-controller/build.rs` bakes configuration into the binary; there is no runtime configuration.

- `fan-controller/.env` (gitignored) must define `FAN_CONTROL_WIFI_NETWORK`,
  `FAN_CONTROL_WIFI_PASSWORD`, `FAN_CONTROL_MQTT_BROKER_USERNAME`,
  `FAN_CONTROL_MQTT_BROKER_PASSWORD`, `FAN_CONTROL_MQTT_BROKER_ADDRESS`,
  `FAN_CONTROL_MQTT_BROKER_PORT`. The build script fails without them. They are re-exported as
  `cargo:rustc-env` and read back through `env!()` in `src/configuration.rs`.
- The Home Assistant MQTT discovery payload is *generated at build time* by
  `set_discovery_payload()`, which builds a `home_assistant_discovery::DiscoveryPayload` from
  `topic` constants, serializes it to JSON, and exposes it as `FAN_CONTROLLER_DISCOVERY_PAYLOAD`.
  The firmware publishes that string verbatim on boot. Changing MQTT topics or discovery fields
  means editing `topic/src/lib.rs` and/or `build.rs` — not the firmware.
- `discovery_payload.json` and `setup_discovery_payload()` are an *alternative, currently disabled*
  path (commented out in `build.rs::main`) that minifies a hand-written payload using Home
  Assistant's own abbreviation table. It reads
  `fan-controller/home-assistant/core/homeassistant/components/mqtt/abbreviations.py`, a submodule
  that is not checked out, so re-enabling it requires fetching that repo first.
- The build script also embeds the short git hash as semver build metadata in the reported
  software version, and re-runs on `../.git/HEAD` changes.

## Workspace crates

| Crate | Target | Purpose |
|---|---|---|
| `fan-controller` | `thumbv6m-none-eabi` | The firmware. Everything below supports it. |
| `mqtt` | `no_std` | Protocol-level MQTT types shared between firmware and build script. Feature-gated `defmt` / `serde` so the same types work on device and on host. |
| `topic` | `no_std` | The single source of truth for Home Assistant MQTT topic strings, composed at compile time with `const_format`. Used by both the firmware and `build.rs`. |
| `home_assistant_discovery` | host | Serde model of the Home Assistant MQTT discovery payload. Build-dependency only. |
| `debug-listener` | host | Reads the RS-485/Modbus line off a USB serial adapter to inspect fan traffic. The port path is hardcoded in `src/main.rs`. |
| `exponential_distribution` | host | Scratch experiment for randomizing MQTT keep-alive ping intervals. Not wired into anything. |

Note `mqtt` appears twice in the firmware: the workspace crate (`::mqtt`) holds protocol constants,
while `fan-controller/src/mqtt/` (`crate::mqtt`) holds the packet encode/decode and client task.

## Firmware architecture

`main()` in `fan-controller/src/main.rs` destructures the RP2040 peripherals, declares every
synchronization primitive as a `static`, and spawns tasks that communicate only through them.
Nothing shares mutable state directly.

Pin assignments live in that destructuring: PIN_4 Modbus driver-enable, UART0 on PIN_12/PIN_13,
PIN_18 button, PIN_20/PIN_21 status LEDs, PIN_23/25/24/29 + PIO0 + DMA_CH0 for the CYW43 Wi-Fi chip.

The primitive type encodes the intent, so pick deliberately when adding one:

- `Channel` (`IN` / `OUT`, size 8) — MQTT publishes in and out; every message must be delivered.
- `Signal` (`FAN_ONE_STATE` / `FAN_TWO_STATE`) — the *requested* set point; only the latest matters.
- `Watch` (`FAN_ONE_DISPLAY_STATE` / `FAN_TWO_DISPLAY_STATE`, 2 receivers each) — the *confirmed*
  set point, published only after the fan acknowledged the Modbus write, and fanned out to both the
  display routine and the button routine.
- `OnceLock` (`FANS`) — the Modbus client, so fan tasks await initialization rather than race it.

Flow of a speed change:

1. `mqtt_routine` brings up the network stack, connects to the broker, and pushes decoded publishes
   into `IN`. `input_routine` is the alternative source: it debounces the button and cycles
   off → low → medium → high from the *minimum* of both fans' current states.
2. `mqtt_brain_routine` interprets `IN` in fan-controller terms and signals `FAN_*_STATE`. It keeps
   `last_fan_state` so a Home Assistant "on" restores the previous speed. `is_synchronization_on`
   (hardcoded `true`) makes a command to either fan drive both.
3. `fan_control_routine` (pool of 2, one per fan address) awaits its `Signal`, writes the holding
   register over Modbus, and only on success sends the value to its display `Watch`.
4. `display_routine` debounces both watches by 250 ms so the two fans' updates land as one, then
   drives `LED_STATE` and publishes state back to MQTT via `OUT`.
5. `led_routine` renders `LedState`; an in-flight animation is cancelled when the state changes.

The `Publish` trait (`task.rs`) plus `TryEncode`/`TryDecode` (`mqtt/mod.rs`) let outgoing messages
be encoded straight into the TCP buffer without intermediate allocation — there is no allocator.

## Domain constants

- Set points are 0..=64_000 (`fan::set_point::MAX`), wrapped in the `SetPoint` newtype.
- User-facing speeds are deliberately *not* the full range — `fan::user_setting::{LOW, MEDIUM,
  HIGH}` are tuned to the house and cap at 50 % to reduce wear. Home Assistant is told
  `speed_range_max: 32_000` to match.
- Fan Modbus addresses start at `0x02`/`0x03`; `0x01` is avoided as a likely factory default.
- UART is 19_200 baud, 8 data bits, **even** parity, 1 stop bit.

## Known-broken state

Both test modules currently fail to compile and are marked with TODOs in the source:

- `home_assistant_discovery/src/lib.rs` tests reference `topic::fan_controller::fan_1::STATE`, but
  the `topic` crate moved those into a `state` submodule (`fan_1::state::STATE`).
- `fan-controller/src/fan/set_point.rs` tests reference `fan::MAX_SET_POINT` and `SetPoint::MAX`,
  neither of which exists, and they cannot run on the embedded target anyway.

Do not assume `cargo test` is green before touching these.

## Reference documents

- [fan-controller/documentation.md](fan-controller/documentation.md) — the LED status protocol
  (which blink pattern means which fan speed / out-of-sync state) and the Home Assistant
  onboarding sequence. Update it when LED behaviour changes.
- [fan-controller/README.md](fan-controller/README.md) — the working TODO list for the firmware.
- [README.md](README.md) — probe firmware updates and where Home Assistant logs rejected discovery
  payloads.
