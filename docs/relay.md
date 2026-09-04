# Modbus relay module

A Shenzhen LC / Elesai **LC-Modbus-1R-D7**: one relay output, one opto-isolated input, speaking
Modbus RTU over RS-485 or a TTL UART. Not yet wired to the controller — this is what talking to it
from a desktop established, so the firmware does not have to rediscover it.

The manufacturer's manual is [docs/manufacturer/relay](manufacturer/relay), in the private
submodule, and [serial/src/devices/relay.ts](../serial/src/devices/relay.ts) models the device from
it. Everything below is our own observation of the module on the bench, which is a different thing
and belongs in this repo: the model says what the manual claims, this says what the hardware did.
They agree on the defaults, which is worth knowing — it means the manual was transcribed correctly
for this device.

## As it arrived

Verified 2026-09-04 against the module on a USB RS-485 adapter, by reading its address, baud code
and coil state and checking the CRC on each answer.

| Setting | Value | How it is known |
|---|---|---|
| Device address | `0xFF` (255) | `00 03 00 00 00 01` → `00 03 02 00 FF` |
| Baud | 9600 | `FF 03 03 E8 00 01` → `FF 03 02 00 03`, code `0x03` |
| Framing | 8 data bits, **no** parity, 1 stop bit | Answers at 8N1, silent at 8E1 |
| Relay | Open, de-energized | `FF 01 00 00 00 08` → `FF 01 01 00` |

All of that is the factory default, so the module is untouched. Address and baud are both stored in
flash and survive a power cycle, so anything changed here is changed permanently.

The address read is worth keeping in the toolbox: `00 03 00 00 00 01 85 DB` is answered whatever
address the module is set to, so it finds a module whose address has been forgotten without
sweeping all 255 of them.

## Useful frames

Addressed to `0xFF`, CRC included. The module echoes writes back verbatim, which is how a write is
confirmed.

| Intent | Send | Expect |
|---|---|---|
| Relay on | `FF 05 00 00 FF 00 99 E4` | the same bytes back |
| Relay off | `FF 05 00 00 00 00 D8 14` | the same bytes back |
| Read relay state | `FF 01 00 00 00 08 28 12` | `FF 01 01 <bitmap>`, bit 0 is this relay |
| Read opto input | `FF 02 00 00 00 08 6C 12` | `FF 02 01 <bitmap>` |
| Read address | `00 03 00 00 00 01 85 DB` | `00 03 02 00 <address>` |
| Read baud | `FF 03 03 E8 00 01 11 A4` | `FF 03 02 00 <02=4800, 03=9600, 04=19200>` |
| Set baud to 19200 | `FF 10 03 E9 00 01 02 00 04 CA 0E` | `FF 10 03 E9 00 01 C5 A7` |

Note the baud rate is read from `0x03E8` but written to `0x03E9` — two addresses for the one
setting. That is not a transcription slip; it is what the manual's own worked examples do, and the
read above was confirmed against the hardware.

There is also a flash mode that closes or opens the contact for a set time on its own, in units of
0.1 s, up to about 6553 s. Any use of it puts a timeout in the relay that the controller cannot see
or cancel, so prefer holding the state from the firmware where the state is known.

## The power-up banner

> [!IMPORTANT]
> On power-up the module sends this down the line, unasked, before anything has requested
> anything:
>
> ```
> Thank you for using the Modbus modules of LCTECH\r\n
> ```

It is not a Modbus frame and it does not belong to any request. Observed on the first exchange
after power-up: it collided with a coil write, the echo came back as
`FF 05 00 00 FF 80 0A 54 68 61 6E 6B ...` instead of `FF 05 00 00 FF 00 99 E4`, and the write was
dropped — the relay stayed off. Every exchange after that was clean, and three on/off cycles ran
without a single bad frame.

Two consequences for firmware that talks to this module:

- Validate the CRC and resynchronize on it. Code that assumes the bytes arriving after a request
  are that request's answer will mis-frame, read the banner as a response, and be wrong about
  whether the relay switched.
- Tolerate a dropped first exchange rather than flushing once at startup and trusting the line
  afterwards. The banner arrives whenever the *module* is powered, which is not the same moment the
  RP2040 boots — a brown-out on the relay's own supply produces it again with the controller
  already running and none the wiser.

The [serial tool](../serial) already does both. Its frame scan was never troubled by the banner —
it searches for a valid frame at every offset, and ASCII cannot be mistaken for the relay's address
of `0xFF` — but the *spoiled* exchange leaves no frame to find at all, and that read used to fail
as "no reply", which reads like a wrong address or a dead bus. It now separates a line that stayed
silent from one that carried bytes it could not use, retries only the second, and quotes the stray
bytes when they are legible, so the greeting names itself in the error.

The firmware's Modbus client no longer assumes the first two bytes after a request are the header
either. It slides a two-byte window forward until the address and function code are the ones it
asked for, bounded by 80 bytes and by the 3.5 characters of silence that end a burst, so a greeting
in front of an answer costs a few milliseconds instead of the whole transaction. That is groundwork
rather than a fix in use: this module cannot join the fans' bus at all, because they run 8E1 and it
only answers 8N1, so nothing the controller drives today can hear the greeting.

## Why it cannot share the fan bus

The fans run 19_200 baud, 8 data bits, **even** parity, 1 stop bit. The relay runs 8N1 and its
manual never mentions parity at all; it answered nothing at 8E1 at any of the three baud rates.
Baud is settable to 19200, parity is not settable at all, so the two cannot be made to agree.

A shared RS-485 segment is therefore off the table as the hardware stands, and the relay wants a
second UART rather than a place on the fan pair. If some later revision does put them together,
re-address the relay off `0xFF` first — the fans deliberately start at `0x02`/`0x03`, skipping
`0x01` as a likely factory default, and `0xFF` is a likely default for the same reason.

## Talking to it from a desktop

The module hangs off a USB RS-485 adapter, `/dev/cu.usbserial-160` on this machine, the same kind
of adapter [debug-listener](../debug-listener) uses to watch the fans. That path changes between
machines and between plug-ins.

The [serial tool](../serial) is the normal way to do this — it opens the adapter from the browser
and already knows this device's registers. The frames above are what to fall back on when talking
to the module from a script instead.

> [!NOTE]
> macOS refuses the port with `Errno 16 Resource busy` while anything else holds the matching
> `/dev/tty.*` — a browser tab with an open Web Serial connection is the easy one to forget.
> `lsof /dev/tty.usbserial-160` names the process.
