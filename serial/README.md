# serial

A browser tool for talking to the RS-485/Modbus RTU devices on this project's bus, using the
[Web Serial API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API). It opens a USB
serial adapter straight from the page, reads input registers and reads *and writes* holding
registers — which is how a device gets configured before it is wired into the fan controller.

It was folded in from [SantaClaas/serial](https://github.com/SantaClaas/serial), where it lived as
its own repository.

## Running it

```bash
cd serial && npm install && npm run dev
```

The Web Serial API is Chromium-only (Chrome, Edge, Opera) and needs a secure context, so `localhost`
or HTTPS. The app says so itself when the API is missing rather than failing silently.

`npm run check` type-checks the Svelte components.

## What it does today

- **Add Port** calls `navigator.serial.requestPort()` filtered to USB vendor `0x1A86` / product
  `0x7523` — the CH340 adapter. That filter is hardcoded in `src/lib/SerialPortsList.svelte`, so a
  different adapter will not show up in the picker.
- Baud rate and parity are chosen per port before opening it. The RadiCal fans want 19 200 baud with
  **even** parity; the temperature sensor defaults to 9600 with **no** parity. A port can only speak
  one of those at a time.
- Devices are added to an open port by Modbus address. Each device type declares its registers
  once — address, length, how to decode the bytes, and for holding registers how to encode and
  validate them — and the UI is generated from that declaration
  (`src/lib/modbus.ts`, `src/lib/devices/`).
- Two device types are implemented: the ebm-papst RadiCal fan (`devices/fan.ts`) and the temperature
  sensor (`devices/temperatureSensor.ts`).

## Where the device documentation lives

- **RadiCal fans** — `docs/manufacturer/radical/`, in the private submodule. The authority on
  register addresses, units and naming.
- **Relay module** — `docs/manufacturer/relay/`, same submodule.
- **Temperature sensor** — [docs/temperature-sensor.md](../docs/temperature-sensor.md). No
  manufacturer PDF exists for it; that file is all there is.

The register names in `devices/fan.ts` are working translations of the German manual made before
that manual was checked in, and several are flagged as guesses in the source (`PhaseControlFactor`
for *Aussteuergrad*, `CurrentDesiredEffect` for *Aktueller Wirksinn*). Prefer the terminology in the
RadiCal documentation over these.

## Status

This is Svelte 4 on Vite 4 and is slated to be rewritten from scratch on SolidJS, adding passive
listening of bus traffic in the manner of [debug-listener](../debug-listener). Treat the current
code as reference for what the devices do, not as the shape the rewrite should take.

The GitHub Pages deployment workflow that shipped with the standalone repository was dropped during
the fold-in: GitHub only reads workflows from the repository root, so it would never have run from
`serial/.github/`. It needs to be re-created at the repository root, scoped to this directory, when
the rewrite is ready to deploy.
