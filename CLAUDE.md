# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

A monorepo for a Raspberry Pi Pico W (RP2040) fan controller that drives two ebm-papst
RadiCal centrifugal fans over Modbus RTU and integrates with Home Assistant over MQTT.
The firmware is `no_std` and built on Embassy. The other crates are supporting libraries and
desktop-side helper binaries.

Most of the repository is one Cargo workspace, rooted at [Cargo.toml](Cargo.toml). Alongside it
sits [serial](serial), a browser tool for the same Modbus bus that is a separate pnpm project and
not a workspace member — see [The serial tool](#the-serial-tool) below.

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

The same holds for [serial](serial), for a different reason — it is a pnpm project rather than a
workspace member, so it is only buildable from its own directory (`cd serial && pnpm dev`).

Tests, checks, and lints run per crate from that crate's directory:

```bash
cd home_assistant_discovery && cargo test
```

```bash
cd home_assistant_discovery && cargo test -- serialize_custom_example
```

`home_assistant_discovery` uses `insta` snapshots (`src/snapshots/`); accept changes with
`cargo insta review`. `fan-controller` has a `bacon.toml`; `bacon` defaults to `cargo check` and `c`
is bound to `clippy-all`. Its `test` job comes from the stock template and cannot work — see
Testing reality below.

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
- The build script also embeds the short git hash as semver build metadata in the reported
  software version, and re-runs on `../.git/HEAD` changes.

## Workspace crates

| Crate | Target | Purpose |
|---|---|---|
| `fan-controller` | `thumbv6m-none-eabi` | The firmware. Everything below supports it. |
| `mqtt` | `no_std` | Protocol-level MQTT types shared between firmware and build script. Feature-gated `defmt` / `serde` so the same types work on device and on host. |
| `topic` | `no_std` | The single source of truth for Home Assistant MQTT topic strings, composed at compile time with `const_format`. Used by both the firmware and `build.rs`. |
| `set_point` | `no_std` | The `SetPoint` newtype and its bounds, parsing and formatting. Its own crate purely so it can be tested on the host; re-exported by the firmware as `crate::fan::set_point`. Feature-gated `defmt`. |
| `fan_sensor` | `no_std` | Decoding what a fan reports about itself — actual speed, both temperatures, power — from its input registers, plus the JSON payload Home Assistant reads. Owns the register addresses and the layout of the two runs that are read. Its own crate for the same reason as `set_point`; re-exported as `crate::fan::sensor`. Feature-gated `defmt`. |
| `home_assistant_discovery` | host | Serde model of the Home Assistant MQTT discovery payload. Build-dependency only. `components` is a `BTreeMap` so the generated payload is byte-stable across builds. |
| `debug-listener` | host | Reads the RS-485/Modbus line off a USB serial adapter to inspect fan traffic. The port path is hardcoded in `src/main.rs`. |

Note `mqtt` appears twice in the firmware: the workspace crate (`::mqtt`) holds protocol constants,
while `fan-controller/src/mqtt/` (`crate::mqtt`) holds the packet encode/decode and client task.

## The serial tool

[serial](serial) is not a crate and not a workspace member — it is a SolidJS 2.0 app on Vite that
talks to the RS-485/Modbus bus from the browser over the
[Web Serial API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API). It uses **pnpm**,
and like the Rust packages it only builds from its own directory:

```bash
cd serial && pnpm install && pnpm dev
```

`pnpm test` runs the protocol tests, `pnpm build` type-checks and builds.

It does two things:

- **Watches the bus** without joining in, decoding the traffic between the fan controller and the
  fans. `debug-listener` reads the same line from the desktop but prints bytes; this frames and
  decodes them.
- **Talks to a device** directly, reading input registers and reading *and writing* holding
  registers — which is how a device is given its address, baud rate and correction values before
  it is wired in. The firmware never writes anything but the set point.

Those two are mutually exclusive on one port: a half-duplex RS-485 line has room for one master,
so the active side must not be used while the controller is polling.

| Path | What it is |
|---|---|
| `serial/src/modbus/` | CRC, frame lengths and decoding, and recovering frames from a bus the app is not driving. Plain TypeScript over bytes, and where the tests are. |
| `serial/src/devices/` | Each device as a list of registers that know their address, their manual's name for them, and how to decode them. The UI is generated from these. |
| `serial/src/serial/` | One open port: a single read loop serving both the monitor and outstanding requests. |
| `serial/src/ui/` | The components. |

Four things to know before changing it:

- **Register names are the manual's own headings, in the manual's own language**, with an English
  gloss beside them and a section number. For the RadiCal that means German — *Aussteuergrad*,
  *Wirksinn*, *Sollwert*. Do not replace them with translations: the old Svelte tool had
  `PhaseControlFactor` for *Aussteuergrad* and `CurrentDesiredEffect` for *Aktueller Wirksinn*,
  both invented, and neither can be looked up in `docs/manufacturer/radical/`.
- **The tests are built on frames copied out of the manufacturers' manuals, check bytes included.**
  They agree only if the code agrees with the devices rather than with itself, which is how two
  wrong CRCs in `docs/temperature-sensor.md` were found. Keep new device work anchored the same
  way.
- **Solid 2.0 is not Solid 1.x.** `createEffect` takes a compute *and* an effect function; there
  is no `onMount`, because a component body already runs once during setup; DOM rendering is in
  `@solidjs/web`, which is also the `jsxImportSource`.
- **Register write fields rely on the browser's own validation.** The bounds are real
  `min`/`max`/`step` attributes taken from the register definition, and `:user-invalid` styles them
  only after the field is left. Do not add a parallel bounds check in a signal.

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

Independently of that flow, `sensor_routine` (pool of 2, one per fan address) polls what the fans
measure about themselves every `SENSOR_POLL_INTERVAL` and publishes it through `OUT`. It shares the
Modbus mutex with `fan_control_routine` and yields to it: a failed poll is logged and dropped
rather than retried, because the next poll carries fresher values than a retry would and a silent
fan would otherwise hold the mutex through a run of timeouts. The fan's maximum speed
(`D119`, a holding register) is read once and cached, because every speed the fan reports is a
fraction of it; until it is known the reading reports the speed as `null` rather than withholding
the other three values. The documented energy counter (`D029`/`D02A`) is *not* read: both fans
answer `0xFFFF` for it, so the sensor was removed rather than announced as a value that is never
real.

The `Publish` trait (`task.rs`) plus `TryEncode`/`TryDecode` (`mqtt/mod.rs`) let outgoing messages
be encoded straight into the TCP buffer without intermediate allocation — there is no allocator.

## Domain constants

- Set points are 0..=64_000 (`set_point::MAX`, re-exported as `fan::set_point::MAX`), wrapped in
  the `SetPoint` newtype.
- User-facing speeds are deliberately *not* the full range — `fan::user_setting::{LOW, MEDIUM,
  HIGH}` cap at 50 % to reduce wear. Home Assistant is told `speed_range_max: 32_000` to match, so
  the range it shows is that capped one. `LOW` and `MEDIUM` are the thirds of it (`MAX / 6` and
  `MAX / 3`), which makes the button cycle through the same steps the Home Assistant slider shows
  rather than through arbitrary points on it.
- Fan Modbus addresses start at `0x02`/`0x03`; `0x01` is avoided as a likely factory default.
- UART is 19_200 baud, 8 data bits, **even** parity, 1 stop bit.
- Sensor values live in *input* registers (function code `0x04`), which are read only, unlike the
  holding registers (`0x03` / `0x06`) the set point lives in. `ReadInputRegisters<COUNT>` asks for a
  range rather than one register, because a range costs the same round trip; the fan refuses more
  than 37 registers or an answer over 80 bytes.
- The Home Assistant discovery payload is encoded into a fixed `mqtt::task::SEND_BUFFER_SIZE`
  buffer. `main.rs` asserts at compile time that it still fits, because the encoder refuses an
  oversized packet and only logs it — which would leave a device that runs fine and is never
  discovered.

## Testing reality

`fan-controller` cannot be tested by any normal means, so don't waste time trying: `cargo test`
targets `thumbv6m-none-eabi`, which has no test harness, and `cargo test --target
aarch64-apple-darwin` fails because `cortex-m` uses ARM inline assembly. A `#[cfg(test)]` module
anywhere in `fan-controller/src/` is compiled by nothing and will rot unnoticed — `set_point` used
to be one and did.

Tests therefore only exist in the crates that build for the host:

```bash
cd set_point && cargo test
```

```bash
cd home_assistant_discovery && cargo test
```

```bash
cd fan_sensor && cargo test
```

That is also the way to make firmware logic testable at all: move it into its own `no_std` crate
and re-export it, the way `fan/mod.rs` re-exports `set_point`. Worth doing for anything with rules
of its own; not worth it for code that only exists to drive a peripheral.

`serial` has its own suite, which runs on the host without a browser because everything under
`src/modbus` and `src/devices` is plain TypeScript over bytes:

```bash
cd serial && pnpm test
```

## Reference documents

- [fan-controller/documentation.md](fan-controller/documentation.md) — the LED status protocol
  (which blink pattern means which fan speed / out-of-sync state) and the Home Assistant
  onboarding sequence. Update it when LED behaviour changes.
- [fan-controller/TODO.md](fan-controller/TODO.md) — the working TODO list for the firmware:
  every outstanding item with source line references and a suggested priority order. Keep it in
  sync when adding or resolving a `//TODO` in `fan-controller/src/`.
- [README.md](README.md) — probe firmware updates and where Home Assistant logs rejected discovery
  payloads.
- `docs/manufacturer/` — a submodule pointing at the private
  [fan-documentation](https://github.com/SantaClaas/fan-documentation) repo, holding the
  manufacturer material kept out of this public repo: the ebm-papst RadiCal MODBUS specification
  (`radical/`) and the Modbus relay module manual (`relay/`). `git submodule update --init` after
  cloning; it needs access to that private repo. This is the authority on RadiCal register
  addresses, units and naming.
- [docs/temperature-sensor.md](docs/temperature-sensor.md) — the Modbus registers and protocol of
  the RS-485 temperature/humidity sensor. No manufacturer PDF exists for that device, so this
  file, and the raw text it was formatted from next to it, is the only documentation there is.
- [docs/relay.md](docs/relay.md) — the LC-Modbus-1R-D7 relay module as measured on the bench rather
  than as documented: the address and line settings it shipped with, the frames that were actually
  exercised against it, the ASCII banner it sends on power-up, and why its 8N1 framing keeps it off
  the fans' 8E1 bus. `serial/src/devices/relay.ts` models the same device from the manual; this
  records where the hardware was checked against that.
