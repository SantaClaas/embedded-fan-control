# Documentation

Not all of the information but I hope to write down most of it.

## Goal statement

Create a fan controller that is reliable and low power to control the fans for our house.
It has to be controllable manually through buttons or dials. Everything else like Homeassistant integration is optional. It is important that it works alone.

## Status LEDs
When the device powers on or after a restart it flashes both lights for a second to help spot eventually broken LEDs.
The status LEDs indicate the fans speed.
### Static LEDs
Both fans run at the same speed if the LEDs are not blinking.

| LED 1 | LED 2 | Status |
|-------|-------|---------------------------|
| Off | Off | Fan and/or controller off |
| On | Off | Fan speed low |
| Off | On | Fan speed medium |
| On | On | Fan speed max |

### Blinking LEDs

Both LEDs blink switching in a 250ms rythm at the start of the controller while the initial fan speed data is getting read from the fan.
Otherwise the LEDs only blink when they are running out of sync at different speeds.

LED 1 indicates fan 1 state.
LED 2 indicates fan 2 state.

If an LED is off, then the fan for that LED is off.
If the LED blinks once and then takes a 5 second break the fan for that LED runs at low speed. Two blinks for medium speed and three blinks for high speed.

> [!NOTE]
> Both LEDs might blink the same number of times. This means they still run at different speeds but within the same range for low, medium or high.


## Homeassistant integration

### 1. Join WiFi

The controller automatically joins the WiFi network that is configured.
The network name and passowrd is currently configured through environment variables at build/compile time and gets flashed onto the device. Plan is to make it configurable through a web interface. But there is no guarantee this will work out.

### 2. Homeassistant discovery

After successfully joining the network it tries to look up Homeassistant under the `homeassistant` name and tries to connect to it.
Homeassistant needs to have the MQTT broker installed as the controller uses MQTT to connect to homeassitant and send data between them.
After successful connection to the MQTT broker, the controller sends a discovery packet as defined by Homeassistant and the device should appear in Homeassistant on the dashboard when using the default Homeassistant configuration.

### 3. Sensors

Alongside the two fans the device announces four sensors per fan: speed in rpm, motor temperature,
electronics temperature, and power draw in watts. The fans are polled every 30 seconds, starting
10 seconds after boot so the initial fan speed read has the bus to itself.

There is deliberately no energy sensor. The Modbus specification documents a consumption counter in
kWh at `D029`/`D02A`, but both fans return `0xFFFF` for both registers, which is what an ebm-papst
fan reports for a register its hardware variant does not implement. Announcing it put a permanent
4 294 967 295 kWh in Homeassistant's energy dashboard, so the sensor was dropped rather than
published as a value that never becomes real.

Speed shows as unknown until the controller has read the fan's configured maximum speed, which
every speed the fan reports is a fraction of. It retries that read on each poll, so a fan that was
unreachable at boot fills in on its own. The other three values do not depend on it and appear
right away.

### 4. Bypass

The device also announces a switch called Bypass, which opens and closes the summer bypass damper
through a relay on the same Modbus bus as the fans. `ON` is open.

The relay is powered separately from the controller, so it keeps its position while the controller
resets. The controller reads that position back about ten seconds after boot rather than assuming
one, which is why the switch can sit unavailable for a few seconds and then appear already on. The
fans get the bus first: their set point reads are what the house needs and the damper can wait.

The switch only moves once the relay has acknowledged the write. If the relay cannot be reached the
firmware retries three times with a growing pause, then gives up and leaves the switch showing the
last position the relay confirmed — it does not show a position the damper is not in. Nothing else
on the device displays the bypass; the two status LEDs are fully spoken for by the fan speeds.

There is deliberately no automation here. When the bypass should open is a question about outdoor
and indoor temperature that Home Assistant already has the sensors for, so the controller exposes
the damper and leaves the decision there.

## Wiring

Every pin the firmware uses is baked into the binary. There is no runtime configuration, so moving
a wire means editing the peripheral destructuring at the top of `main.rs` and flashing again.

```mermaid
flowchart LR
    BUTTON[Button<br/>momentary, to GND] -- GP18 --> PICO
    PICO -- GP21 --> LED1[LED 1, fan 1]
    PICO -- GP20 --> LED2[LED 2, fan 2]
    PICO[Raspberry Pi Pico W] -- "GP4 to DE/RE<br/>GP12 to DI, GP13 to RO<br/>3V3 and GND" --> TRANSCEIVER[RS-485 transceiver]
    TRANSCEIVER -- "A and B, twisted pair" --> FAN1[Fan 1, address 0x02]
    FAN1 -- "the same pair, daisy chained" --> FAN2[Fan 2, address 0x03]
    FAN2 -- "and on again" --> RELAY[Bypass relay, address 0x04]
    PROBE[Debug probe, optional] -. "SWCLK, GND, SWDIO" .-> PICO
```

### Pin assignment

| Pico pin | GPIO | Direction | Net | Wired to |
|---|---|---|---|---|
| 6 | GP4 | Output, idle low | `MODBUS_DE` | DE and RE on the transceiver, tied together |
| 16 | GP12 | UART0 TX | `MODBUS_TX` | DI |
| 17 | GP13 | UART0 RX | `MODBUS_RX` | RO |
| 24 | GP18 | Input, internal pull-up | `BUTTON` | One side of the button, the other side to GND |
| 26 | GP20 | Output, active high | `LED_2` | LED 2 anode through a series resistor, cathode to GND |
| 27 | GP21 | Output, active high | `LED_1` | LED 1 anode through a series resistor, cathode to GND |
| 36 | 3V3(OUT) | Supply out | `+3V3` | Transceiver VCC |
| 38 | GND | — | `GND` | Transceiver GND, LED cathodes, button, RS-485 common |
| On the module | GP23, GP24, GP25, GP29 | PIO0 + DMA0 | CYW43439 | Nothing. The Wi-Fi chip sits on the Pico W itself |

Pin numbers are physical positions on the board, GPIO numbers are what the firmware calls them.
Any of the eight ground pins will do; 38 is just the one nearest the signals on that side.

### RS-485 to the fans

The fans speak Modbus RTU on a two-wire bus, which is half duplex: the same pair carries the
request and the answer, so only one device may drive it at a time. GP4 is what arbitrates that. It
idles low, which leaves the transceiver receiving and the fans owning the line, and goes high only
for the few hundred microseconds a request takes.

> [!IMPORTANT]
> Use a transceiver rated for 3.3 V, such as a MAX3485 or an SN65HVD72. A real MAX485 is a 5 V
> part and its RO output would then swing to 5 V into GP13, which is not 5 V tolerant. Many of the
> cheap blue breakout boards sold as "MAX485 modules" are 5 V only.

- Wire it as a bus, not a star: one pair from the transceiver to fan 1, on from fan 1 to fan 2, and
  on again to the bypass relay.
- Terminate both ends with 120 Ω across A and B, one at the transceiver and one at the last device
  on the chain, and nothing in between.
- A and B are a twisted pair, with a third conductor tying the fans' RS-485 common back to
  controller ground. A differential pair still needs both ends to agree where zero is.
- The fans have to be set to 19_200 baud, 8 data bits, even parity, 1 stop bit. Even parity in
  particular is easy to leave on the wrong setting.
- Addresses are set on the fans themselves: `0x02` for fan 1 and `0x03` for fan 2. `0x01` is
  skipped because it is a likely factory default, and `0x04` is the bypass relay.

When the frame has been written, `blocking_flush()` returns as soon as the software buffer is
empty, but the frame is still in the hardware FIFO and shift register rather than on the wire. So
`send_request` in `modbus/client.rs` spins on `uart.busy()` and only then drops GP4. Dropping it
early truncates the frame, the fan rejects it on the checksum, and the result looks exactly like a
fan that is not answering. The line has to be back in the fan's hands well within the 3.5
characters of silence it waits before replying, which is about 2 ms at this baud rate.

### Bypass relay

The bypass damper hangs off an LC Technology `LC-Modbus-1R-D7`, a single relay module that speaks
Modbus RTU over the same RS-485 pair as the fans. It is the last device on the bus, so the 120 Ω
termination that used to sit at fan 2 moves to the relay. It takes its own DC 7–24 V supply; none
of that passes through the Pico.

> [!WARNING]
> **Parity is unverified.** The bus runs 19 200 baud, 8 data bits, even parity, 1 stop bit. The
> relay's manual lists three baud rates and never mentions parity, and modules of this kind are
> usually 8N1 — which cannot share a line with 8E1 in either direction, because the parity bit
> lands where the receiver expects the stop bit. If the relay does not answer once it is on the
> bus, this is the first thing to suspect, and the fallback is to give it its own UART and
> transceiver at 9600 8N1 rather than to change the fans.

Commission it on the bench, before it goes on the bus with the fans:

- **Set the address to `0x04`.** It ships as `0xFF`, which Modbus reserves rather than gives to a
  device, and `0x04` carries on from the fans at `0x02` and `0x03`. Write it to holding register
  `0x0000` with function `0x10`.
- **Set the baud rate to 19 200.** It ships at 9600. Write `0x04` to holding register `0x03E9`,
  again with function `0x10`. Note the manual reads the baud rate back from `0x03E8`, one below
  where it is written — that asymmetry is what the manual says, not a typo here.

The firmware drives coil `0x0000`, which is the module's only relay. Do not confuse it with holding
register `0x0000` on the same device: that one is the module's address, and writing a bypass
position there would renumber the module instead of moving the damper.

#### Which contact the damper hangs off

The firmware's convention is that an **energised relay means the bypass is open**. The module has
three terminals — `NO`, `COM` and `NC` — and which pair the damper actuator sits across decides
where it ends up when the relay loses power, which the firmware cannot see or control:

| Damper wired across | Relay unpowered | Consequence |
|---|---|---|
| `COM` and `NO` | Bypass closed | Heat recovery, the safe winter default |
| `COM` and `NC` | Bypass open | Heat recovery lost until power returns |

`COM` and `NO` is the conservative build: a relay that loses power, or a controller that never
comes back, leaves the house recovering heat rather than venting it. Note this is about the
*relay's* supply, not the controller's — the relay holds its position through a controller reset,
which is exactly why the firmware reads it back on boot instead of assuming.

### Button

GP18 has the internal pull-up on and the firmware acts on the falling edge, so the switch just
shorts GP18 to ground and needs no external resistor. Debouncing is 250 ms in software, so a plain
momentary switch is enough and there is no need for an RC network.

### Status LED wiring

Both outputs are active high: the pin drives the anode through a series resistor and the cathode
goes to ground. Around 330 Ω gives a comfortable few milliamps at 3.3 V. LED 1 is GP21 and reports
fan 1, LED 2 is GP20 and reports fan 2 — crossing the two makes the blink patterns above describe
the wrong fan.

### Power

The Pico runs from USB or from a supply on VSYS, and the transceiver is the only thing hanging off
3V3(OUT). The fans have their own mains supply and none of it passes through this board; only the
RS-485 pair and its ground reference cross between the two.

### Debug probe

Optional and only for development: three wires to the SWD header on the bottom edge of the Pico W,
SWCLK, GND and SWDIO, from a debug probe or a second Pico running picoprobe. `cargo run` flashes
through it and the `defmt` logs come back over the same connection.

### What is convention rather than fixed

The GPIO column above is compiled in. The rest is ordinary practice and worth knowing as such:

- The resistor values, 330 Ω for the LEDs and 120 Ω for termination, are the usual starting points
  and were not measured for this build.
- Powering the transceiver from 3V3(OUT) assumes a 3.3 V part, as above.
- No isolation is shown. For a long run through the house an isolated transceiver is the more
  conservative build.
