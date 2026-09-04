# serial

A browser tool for the RS-485/Modbus RTU bus this project's devices sit on, using the
[Web Serial API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API). It does two
things a firmware build cannot:

- **Watches the bus.** Decodes the traffic between the fan controller and the fans without joining
  in, the way [debug-listener](../debug-listener) does from the desktop — but framed and decoded
  rather than printed as bytes.
- **Talks to a device directly.** Reads input registers and reads *and writes* holding registers,
  which is how a device gets its address, baud rate and correction values set before it is wired in.

Only one of those at a time per port: watching is passive and polling is not, and a half-duplex
RS-485 line has room for one master.

## Running it

```bash
cd serial && pnpm install && pnpm dev
```

`pnpm test` runs the protocol tests, `pnpm check` type-checks, `pnpm build` does both and produces
`dist/`.

## What it remembers

The page is static and gets reloaded often — to pick up a new build, or because an adapter was
unplugged — and every choice it offers is one the user made last time and would make again. So each
port's settings, which of the two tabs it was on, and the device and address last talked to are
kept in `localStorage` under a key derived from the adapter's USB ids, and restored when that port
comes back. `src/ui/preferences.ts` is that, and its parsing is deliberately forgiving: a stored
record outlives the build that wrote it, so every field is checked and falls back on its own.

Nothing that was *read* is remembered. A register value is a statement about the device as it was,
and a restored one sitting beside a live one would make the page lie about what it knows.

Reopening the port on load is off until asked for, per port, with the checkbox beside **Open
port**. Opening a port asserts the adapter's control lines, and on an adapter that drives the
RS-485 transceiver from RTS that is the one thing a tool meant to be passive must not do behind the
user's back.

## Deploying it

[`.github/workflows/serial-pages.yml`](../.github/workflows/serial-pages.yml) builds this directory
and publishes it to GitHub Pages on every push to `main` that touches `serial/`. It runs the tests
and the type-check first, and it runs on pull requests too without deploying.

Pages has to be switched on for the repository once before the first deploy:
**Settings → Pages → Build and deployment → Source: GitHub Actions**.

The build sets Vite's `base` to `./`, so the same `dist/` works at a domain root and under the
`/<repo>/` path a project site is served from.

The Web Serial API is Chromium-only (Chrome, Edge, Opera) and needs a secure context, so `localhost`
or HTTPS. The app says so itself rather than failing silently.

## Layout

| Path | What it is |
|---|---|
| `src/modbus/crc.ts` | The CRC-16 every RTU frame ends with. The one little-endian field in the protocol. |
| `src/modbus/pdu.ts` | Frame lengths, decoding and building, per function code. |
| `src/modbus/monitor.ts` | Recovering frames from a bus you are not driving. |
| `src/modbus/frames.fixture.ts` | Frames copied out of the device manuals, CRCs and all. |
| `src/ui/preferences.ts` | The choices kept between visits, and the parsing that survives an older build's record. |

Everything under `src/modbus` is plain TypeScript over bytes with no DOM in it, so it is tested
directly with `vitest`. The tests are built on frames the *manufacturers* printed, which is the
strongest check available: they agree only if this code agrees with the devices rather than merely
with itself. `preferences.ts` takes its storage as an argument for the same reason, so what it
makes of a malformed record can be checked without a browser.

> That is not hypothetical. The humidity frames in
> [docs/temperature-sensor.md](../docs/temperature-sensor.md) had wrong check bytes, and the test
> comparing them against `crc.ts` is what found it. Both are corrected there now.

## Where the device documentation lives

- **RadiCal fans** — `docs/manufacturer/radical/`, in the private submodule. The authority on
  register addresses, units and naming.
- **Relay module** — `docs/manufacturer/relay/`, same submodule.
- **Temperature sensor** — [docs/temperature-sensor.md](../docs/temperature-sensor.md). No
  manufacturer PDF exists for it; that file is all there is.

The German terms in the RadiCal manual are the reference for what a register is called — *Aussteuergrad*,
*Wirksinn*, *Sollwert* — rather than any translation invented here.

## Stack

SolidJS 2.0 (release candidate) on Vite, with pnpm. Solid 2.0 differs from 1.x in ways that matter
when reading this code:

- `createEffect` takes **two** functions, a compute and an effect: `createEffect(() => signal(), value => …)`.
  The single-argument form from 1.x is gone and is typed `never`.
- DOM rendering lives in `@solidjs/web`, not `solid-js/web`, and that is also the `jsxImportSource`.
