# TODO inventory

Every outstanding item in the firmware, collected from [README.md](README.md),
[documentation.md](documentation.md), and `//TODO` comments in `src/`.
Paths and line numbers are relative to `fan-controller/`. They drift as the source changes, so
treat them as a starting point rather than an exact address.

The first section is a suggested order of work with the reasoning; the sections after it are the
full inventory grouped by area, so nothing gets lost.

Everything in the priority section is done: the four ranked items, the cheap win, and the sensor
polling that followed them. They are kept here rather than deleted because each one records what was
actually wrong, what was decided, and what has never run on hardware — the write-ups are the closest
thing this firmware has to a changelog with reasons. What follows them is what has been asked for
but not started, and then the unranked inventory to pick from. The one thing worth doing before
anything else is flashing the device and watching the log, because none of the finished items have
run on hardware.

---

## Suggested priority

### P0 — the two items that made the device wrong or dead in the field

**1. Validate the Modbus response** — done, `src/modbus/client.rs`

`Client::send_3` used to end in an unconditional `Ok(())`: it never inspected the response bytes and
never checked how many arrived, so anything that was not a UART error or a timeout counted as
success — a modbus exception, a reply from the *other* fan, or line noise.

That mattered because the whole "confirmed set point" design rests on it. The `Watch` senders are
documented as publishing only after the fan acknowledged the write, and `fan_control_routine`
updates `current_set_point`, the display state, the LEDs, and the Home Assistant state on the
strength of that `Ok(())`.

`send_3` now reads the address and function code first, branches on whether an echo or an exception
frame is arriving, and reads exactly the length of that frame with `read_exact` instead of a
possibly short `read`. A successful write is confirmed by comparing the whole frame against the
request, which validates the register, the value, and the checksum at once. Exception frames are
checksum checked and returned as `WriteError::Exception`, so the existing retry loop now fires for a fan
that answers but refuses. Every failure carries the reason and, where it applies, which part of the
exchange it happened in, so the retry log in `fan_control_routine` identifies the fault without
having to read back through the trace. After any failure the receive buffer is drained, so a partial frame cannot
be read as the answer to the next transaction — both fans share the UART, so leftovers from one
would otherwise alias onto the other.

The vendor specification confirms the frame layout this relies on: the response to `0x06` echoes
the request, an exception sets the MSB of the function code and carries a single code byte, and the
fan stays silent rather than answering when the address, length, or checksum of the request is
wrong. It also documents that the fan ignores the four least significant bits of a set point, which
`LOW` and `MEDIUM` both set, so the echo is compared with those bits masked rather than byte for
byte.

Untested on hardware. The exception, checksum, and incomplete-frame paths have never run, and
`BLOCK_FOR` was changed from 5 ms to 800 µs at the same time — the old value kept the driver
enabled into the window where the fan may already be answering. Both want a look at the log on the
first flash.

Still open in the same area: `src/modbus/client.rs:329`, why the flush must be blocking to avoid
`WouldBlock`.

**2. The MQTT client never reconnects** — done, `src/task.rs`

`mqtt_routine` (`src/main.rs`) called `mqtt_with_connect` exactly once, and the session inside it
had no way to end cleanly: `listen` and `keep_alive` returned on a read error, a timeout or a
closed connection, but `talk` and `handle_publish_send` loop forever, so `join5` never completed
and the task hung on a dead socket. Either way the controller was offline until it was power
cycled, quietly — the button routine is independent and keeps working, and no LED pattern signals
that Home Assistant has been disconnected.

`mqtt_with_connect` is now a loop that runs one session at a time and never returns. Per attempt it
creates a fresh socket, resolves the broker address, connects once, and runs the session; when the
session ends it aborts the socket, flushes so the reset actually goes out, and waits before trying
again. The wait starts at `MQTT_RECONNECT_BACKOFF_INITIAL` (1 s), doubles up to
`MQTT_RECONNECT_BACKOFF_MAX` (60 s), and is reset as soon as an MQTT connection is established, so
a short hiccup recovers immediately and a broker that is down for the evening is not asked every
second. The TCP connect no longer retries on its own — that was the one path that would have
skipped the backoff entirely.

The session is now `select3(listen, keep_alive, join3(talk, set_up_subscriptions,
handle_publish_send))`. Only the first two notice that the connection is gone; selecting on them
drops the other three, which cancels them. That is the `src/task.rs` "cancel all tasks" TODO, and it
is what makes reconnecting possible at all — everything belonging to a session (subscription
acknowledgements, the outgoing channel, the writer mutex) is scoped to it and rebuilt on the next
one, so subscriptions are re-established automatically.

`listen` and `keep_alive` return a `SessionEnd` describing why instead of signalling a
`ClientState` that nothing ever waited on, so the reconnect log says whether the broker closed the
connection, sent a disconnect packet, the socket read failed, a packet could not be parsed, or the
keep alive ping went unanswered. The write-only `ClientState` signal is gone. A received disconnect
packet used to be logged and otherwise ignored, leaving the session to limp on until the keep alive
timed out; it now ends the session like any other lost connection, which is also the `src/task.rs`
"close the TCP connection on `Disconnect`" TODO.

Untested on hardware. Worth watching on the first flash: that a broker restart is actually noticed
and recovered from, and that reusing the socket buffers across attempts does not leave the stack in
a bad state.

Follow-up in the same area: nothing is re-announced after reconnecting. Home Assistant keeps its
discovered entities, so the device does not disappear, but its state is whatever it was before the
drop until the next fan change publishes a new one. Re-publishing the discovery payload and the
current display state on every successful connect needs a signal from the session out to
`display_routine`. P1 item 3 made that worth doing: the display state is now seeded from the fans on
boot, so there is a real state to re-announce rather than a `None`.
Messages that `talk` or `handle_publish_send` had picked up but not yet written are also lost when
the session is cancelled; for state updates where only the latest value matters that is acceptable.

### P1 — state was wrong after every reset

**3. Read the fan speed on boot** — done, `src/modbus/`, `src/main.rs`

After a reset the fans keep spinning at whatever they were set to, but the controller assumed
nothing: `current_set_point` started as `None` and `last_fan_state` at `SetPoint::ZERO`. The LEDs
and the Home Assistant state were wrong until someone issued a command, and a Home Assistant "on"
restored a set point that was never actually in effect.

Reading holding registers (function code `0x03`) is now implemented. `ReadHoldingRegister` asks for
exactly one register, which keeps the response a fixed length, and `Client::read_holding_register`
returns its contents. The parts both functions share are factored out of the write path rather than
copied: `send_request` drives the line and writes the frame, and `read_header` reads the address and
function code and checks an exception frame, reporting a refusal as the raw code because the
specification lists different ones per function. Each transaction then only has to read its own
body. A read is rejected if it announces a byte count other than the one register asked for, which
is checked before the checksum because a different length means the wrong bytes were just read.
`send_3` was renamed to `write_holding_register` to pair with it.

`fan_control_routine` reads the set point before it waits for anything, and sends what it gets into
the display `Watch`, so the LEDs, Home Assistant and the button all start from what the fans are
actually doing. `read_set_point` retries `MAX_ATTEMPTS` times sharing the write path's backoff, and
gives up early if a set point is requested in the meantime — a command is worth more than the state
being read, and an unreachable fan costs a timeout per attempt.

`current_set_point` stays an `Option` rather than becoming a plain `SetPoint` as this item
originally suggested. A fan that does not answer leaves its state genuinely unknown, and every place
that uses it — the redundant-write check and the "push the other fan back" path — already treats not
knowing as its own case. Filling it with a guess would make those two silently wrong instead.

`last_fan_state` no longer starts at zero and no longer records the set points that Home Assistant
asks for. `mqtt_brain_routine` now watches the confirmed display state of both fans, so it is seeded
from the boot read and picks up the speeds set with the button as well, and it only remembers
non-zero speeds since zero is the state "on" restores from. It records what a fan actually accepted
rather than what was asked of it, so a failed write no longer leaves a speed behind that was never
in effect. The display `Watch` went from two receivers to three for this, through a
`DISPLAY_STATE_RECEIVERS` alias so the count lives in one place.

`display_routine` also tells Home Assistant whether the fans are on or off on the first update after
boot. It only published that on a *change*, and the first update is not a change, so the speed
arrived without the on/off state that Home Assistant needs to render it.

The 250 ms alternating LED pattern that `documentation.md` describes as "while the initial fan speed
data is getting read from the fan" needed no work: it is what `led_routine` already plays before it
has an `LedState`. It just describes reality now instead of blinking until the first command.

Untested on hardware. The read path has never run: worth checking on the first flash that the fans
answer `0x03` at all, what they report for a fan that is off, and whether the value comes back with
the four least significant bits the fan ignores zeroed or as they were written.

**4. Fans ping-pong forever when the bus is down** — done, `src/main.rs`

After exhausting `MAX_ATTEMPTS`, a fan signalled the *other* fan back to its own last known good set
point, to keep the two from drifting apart and putting the house under or over pressure. If the bus
itself was down, that fan failed too and signalled back, and neither ever stopped. A slow churn
rather than a spin — roughly four attempts at a 5 s timeout plus backoff per round — but it never
terminated and kept cycling the Modbus mutex.

The signal now carries where the set point came from, as `RequestedSetPoint::FromUser` or
`FromOtherFan`, and only a request from a user pushes the other fan back when it fails. That is
option A from the notes that used to sit here, a retry strategy on the signal set to once, except
the "once" falls out of what the value means rather than being counted.

The reasoning is that a correction which fails is not the two fans disagreeing, it is the bus being
down, and there is nothing left for a second correction to fix: the pushing fan is at its own set
point and the fan being pushed already failed at that exact value. Answering it with another
correction is precisely what bounced the same value back and forth. So a correction is applied and
retried like anything else, but it never produces another correction, and every round of signalling
ends after at most two: one if a single fan failed, two if both were commanded at once and both
failed.

That leaves the fans genuinely out of sync in one case — a fan that fails to apply a correction —
which is the honest outcome, because nothing else can be tried until the bus comes back. It is not
silent: the display state is only updated on a confirmed write, so the two LEDs blink the
out-of-sync pattern until a later command succeeds.

Option B, a counter that detects the loop, is not needed on top of this. It would spot the same
situation later and without saying why it happened.

Untested on hardware, and hard to reach on purpose: it needs both fans to fail after the retries,
which means pulling the bus rather than anything Home Assistant can ask for.

### Done since — sensor polling

**Poll what the fans measure about themselves** — done, `fan_sensor/`, `src/modbus/`, `src/main.rs`

This is the "Read temperature sensors" item from `README.md`, done wider than it was written: the
fans report an actual speed, a motor temperature, an electronics temperature and a current power
draw, and all four now reach Home Assistant. A fifth, the energy counter, was announced at first and
has since been removed — see below.

They live in *input* registers rather than holding registers, so function code `0x04` had to be
implemented. `ReadInputRegisters<COUNT>` asks for a range rather than a single register — the values
worth polling sit next to each other and a range costs the same round trip — and carries the count
in the type so the request and the array it is answered with cannot disagree. `COUNT` is checked at
compile time against the fan's limit of 37 registers, which it otherwise reports as exception `0x03`
saying only that the answer would be the wrong length.

`sensor_routine` polls each fan every 30 s, after a 10 s startup delay that leaves the bus to the
set point both `fan_control_routine`s read on boot. Two reads under one lock, so the four values
describe the same moment. Nothing on the device acts on them, so a failed poll is logged and
dropped rather than retried: the next poll carries fresher values than a retry would, and a fan
that has stopped answering does not hold the Modbus mutex through a run of timeouts while a speed
change waits behind it.

Decoding lives in the `fan_sensor` crate, following the `set_point` pattern, so the rules it has —
a speed that is a fraction of the fan's configured maximum, temperatures that are signed — are
tested on the host. The fan's maximum speed (`D119`) is read once and cached; until it is known the
reading reports the speed as `null`, which Home Assistant shows as unknown, rather than holding back
the three values that do not depend on it.

Two things had to be fixed to make it work at all, both of which were already wrong:

- The discovery payload went from 2 components to 12 and from roughly 1.3 kB to 3.8 kB, and `send`
  encoded every packet into a 1024 byte buffer. An oversized packet is refused by the encoder and
  only logged, so the device would have run perfectly and never been discovered. The buffer is now
  `mqtt::task::SEND_BUFFER_SIZE`, and `main.rs` asserts at compile time that the payload still fits,
  because it grows every time a component is added.
- `send` used `write`, whose return value says how many bytes were actually taken and was discarded.
  Anything past the room left in the socket's send buffer was dropped without a word. It now uses
  `write_all`. This was survivable while every packet was short and is not for the discovery
  payload, which is several times that buffer.

The one thing that came back wrong was the energy counter, which this section had flagged as worth
watching. Both fans answer `0xFFFF` for both of its registers (`D029`/`D02A`), i.e. `4294967295`
kWh in Home Assistant, which is what an ebm-papst fan reports for a register its hardware variant
does not implement — there is no parameter to switch the counter on, and the specification's
foreword warns that the documented feature set depends on the variant. The sensor is gone from the
discovery payload and from `fan_sensor::Reading`; the run at `D027` is now a single register and is
named after the power draw it actually carries. That leaves 10 components in the payload rather
than the 12 described above.

Still open in the same area: the temperature/humidity sensor inputs (`D02E`-`D031`) and the PT1000
inputs (`D038`/`D039`) are not read, because they only report anything if sensors are physically
wired to the fans. The motor status (`D011`) and warning (`D012`) bitfields are read as part of the
status run and thrown away; decoding them would give Home Assistant a real diagnostic instead of
inference from a temperature.

### Cheap win worth slotting in anywhere

**Make `SetPoint` host-testable** — done, `set_point/`

The `#[cfg(test)]` module in `src/fan/set_point.rs` never compiled, and it had already rotted once
and was being kept correct by hand. `SetPoint` now lives in its own `no_std` crate at
`set_point/`, with `defmt` behind a feature so the same type works on the device and on the host,
and `fan/mod.rs` re-exports it so every `fan::set_point::…` path still reads the same.

Its tests run: `cd set_point && cargo test`. The two that were rotting are back, and two more cover
`FromStr`, which every speed Home Assistant asks for goes through and which nothing tested before —
including the payloads that are not set points at all.

This is the pattern for testing anything else in the firmware: move the logic into its own crate
and re-export it. Recorded in `CLAUDE.md`, which also no longer claims `bacon test` works in
`fan-controller`, because that job is from the stock template and the target has no test harness.

### Done since — the bypass damper

A relay on the modbus bus opens and closes the summer bypass, and Home Assistant drives it as a
switch. It went in as three layers: a `Switch` variant in `home_assistant_discovery` with the
topics in `topic/src/lib.rs`, then `WriteSingleCoil`/`ReadCoil` and their client methods in
`src/modbus/`, then `bypass_routine` and `src/bypass.rs`.

Worth recording, because each was a decision rather than an obvious step:

- **Coil addresses are their own type.** On this relay coil `0x0000` is the relay and holding
  register `0x0000` is the module's own device address, so confusing the two would renumber the
  module instead of moving the damper.
- **`transact_write` is shared with the register write.** The frames and the echo are identical;
  only how much of the echoed value has to match differed, so the fan's ignored low bits became an
  argument instead of a constant in the function body.
- **A failed write is not corrected, only logged.** Unlike two fans drifting apart, a damper that
  stayed where it was does not put the house under pressure, so there is nothing to push back.
- **The position is read back on boot.** The relay has its own supply and holds its position
  through a controller reset, so what it reports is the truth rather than a guess. It waits
  `BYPASS_STARTUP_DELAY` first so the fans' set point reads get the bus.
- **The state publish is awaited, not dropped on a full channel.** It is the only message that
  will ever describe that position, unlike a sensor reading that repeats on a timer.
- **No LEDs.** Both are fully spoken for by the fan speed protocol.
- **No automation on the device.** When to open the bypass is a temperature question Home
  Assistant already has the sensors for.

Two things are unverified and both need hardware. **The relay's parity is undocumented** — the bus
is 19 200 8E1 and modules of this kind are usually 8N1, which cannot share the line. If it stays
silent, the fallback is its own UART and transceiver at 9600 8N1 rather than changing the fans.
And the relay has to be **commissioned off the bus first**: address `0xFF` → `0x04`, baud 9600 →
19 200. Both are written up in the bypass relay section of `documentation.md`, along with which
contact to hang the damper off so a relay that loses power leaves the house recovering heat.

---

## Asked for, not yet started

### Break up `main.rs`

`src/main.rs` is 1,396 lines and holds nearly everything that is not a protocol: the button
routine, the display debounce and its MQTT publishing, the LED state machine and its animations,
the MQTT brain that interprets incoming publishes, the fan control routine with its retry and
correction logic, the sensor polling routine, the shared type aliases and statics, and the pin
destructuring in `main()` itself. Reading any one of those means scrolling past the other seven.

The task boundaries are already the seams — the routines only talk through the statics declared in
`main()`, so moving each into its own module costs nothing structurally. What needs deciding is
where the shared vocabulary goes: `RequestedSetPoint`, `LedState`/`Blink`, the `DisplayState*`
aliases, `ModbusMutex`/`ModbusOnceLock`, and `back_off`/`MAX_ATTEMPTS` are used across several
routines. `main()` should end up as the wiring — peripherals, statics, spawns — and nothing else.

`src/task.rs` (778 lines) is the second candidate and may be the bigger win: it is the MQTT
session, the reconnect loop, the subscription acknowledgement machinery and the packet plumbing in
one file, and the acknowledgement cluster listed under *MQTT client* below is a refactor waiting
inside it. `src/modbus/client.rs` (607 lines) is long but coherent — one client, one transaction
per function code — so it is likely fine as it is.

Worth doing before the next feature rather than after, since the item below adds code to
`main.rs` in several places. Nothing here changes behaviour, and nothing in `fan-controller` can be tested, so the
only check is that it still compiles and still runs on hardware — which argues for moving code
verbatim first and cleaning up in separate commits.

### A Home Assistant toggle for running the fans out of sync

Not in there today, in any form, but most of the pieces are. Home Assistant already exposes the two
fans as separate entities, so the per-fan command path exists — it is just overridden.
`is_synchronization_on` is a hardcoded `true` in `mqtt_brain_routine` (`src/main.rs:698`) that
mirrors every speed command onto the other fan, and `fan_control_routine` pushes the other fan back
to its last good speed when a write fails (`src/main.rs:930`), which is a second, independent way
the two are kept equal. Both existing `//TODO` comments there are this item.

The control is a switch in Home Assistant, not a second physical button. The physical button stays
what it is — the thing that puts the fans back together — so pressing it turns synchronization back
*on* in addition to cycling both fans to the same speed. It already signals the same set point to
both (`src/main.rs:104`), so the speeds take care of themselves; what is new is that it has to set
the mode and get the toggle in Home Assistant to follow.

The work, in the order it has to happen:

1. **A switch component in the discovery payload.** — done by the bypass work.
   `home_assistant_discovery::Component` now has a `Switch` variant, and the bypass shows how one
   is announced: topics in `topic/src/lib.rs` next to the controller-wide `STATE`/`COMMAND`, the
   component added in `set_discovery_payload()` in `build.rs`. Synchronization is a property of the
   controller rather than of either fan, so its topics belong in the same place. Watch the payload
   size: `main.rs` asserts at compile time that it fits `mqtt::task::SEND_BUFFER_SIZE`, and the
   bypass switch left about 625 bytes of that. The assertion exists precisely because a payload
   that does not fit is refused and only logged.
2. **A seventh subscription.** `SUBSCRIPTIONS` in `src/task.rs` is a fixed array with a
   `SUBSCRIPTIONS_LENGTH` next to it — six since the bypass — and the command topic joins it. The
   bypass command is the worked example of the rest of this step. Decoding needs a new
   `IncomingPublish` variant alongside `FanCommand` (`src/main.rs:427`) and an arm in the
   `TryFrom<Publish>` match (`src/main.rs:450`) — the payload is `ON`/`OFF`, which
   the existing arms match as raw `b"ON"`/`b"OFF"` bytes inline rather than through a shared
   parser.
3. **Somewhere for the mode to live.** It is read by `mqtt_brain_routine` and needs to be honoured
   by both `fan_control_routine`s in the correction path, which are different tasks, and written by
   both the MQTT brain and the button routine. A `Watch<bool>` with a receiver per reader fits the
   existing vocabulary; an `AtomicBool` would do too but would not let anything wait on a change.
4. **Publish the switch state back.** Home Assistant has to see the toggle flip when the button
   turns synchronization back on, the same way `display_routine` publishes fan state after a
   confirmed write. Whether that publish lives in `display_routine` or next to whatever owns the
   mode is the one structural choice here.
5. **Honour the mode in the correction path** (`src/main.rs:930`). Leaving this out would mean a
   failed write silently re-synchronizes fans that were deliberately set apart.
6. **Snap the fans together when synchronization is turned on.** Decided: turning the toggle on
   takes the *minimum* of both confirmed set points and signals it to both fans, the same base
   state the button picks (`src/main.rs:121`). Without this the toggle changes no set point at all,
   so the fans would sit at whatever speeds they were left at until the next command — turning
   synchronization on and watching nothing happen is the wrong answer. Taking the minimum rather
   than the maximum keeps the house from being pushed harder than either fan was asked for. Worth
   factoring out of `input_routine` so the button and the toggle share one function instead of two
   copies of the same rule.

One thing is still open. The LED protocol already has an out-of-sync pattern, but it means
*unintentionally* out of sync — a failed write — and `documentation.md` describes it that way.
Deliberately unequal fans would blink the error pattern forever unless the two are distinguished.
`documentation.md` needs updating either way, both for the LEDs and for the new entity in the
onboarding sequence.

---

## Full inventory

### Fan state and control logic — `src/main.rs`

| Where | Item |
|---|---|
| `src/main.rs:665` | Make `is_synchronization_on` configurable through a switch (hardcoded `true`) |
| `src/main.rs:898` | Skip pushing the other fan's speed when a setting allows the fans to run out of sync |
| `src/main.rs:834` | Consider updating the display state even when the new set point equals the current one |
| `src/main.rs:294`, `:307`, `:332`, `:345` | Handle backpressure when the MQTT out channel is full |

The four backpressure sites are the same code twice per fan (state update, then speed update) and
currently log an error and drop the publish, so Home Assistant silently misses the update. Since
these are display updates where only the latest value matters, dropping the *oldest* or moving to a
latest-wins primitive fits better than blocking. Worth factoring into one helper while you are there.

### MQTT client — `src/task.rs`

| Where | Item |
|---|---|
| `src/task.rs:46` | Rework subscribe acknowledgement into a channel that sends the packet identifier |
| `src/task.rs:72` | `wait_for_acknowledgement` can hang if called concurrently: one waker plus a `try_lock`. Use `embassy-sync::waitqueue` and/or a blocking mutex |
| `src/task.rs:674` | Replace the static "global" acknowledgement state once wakers and polling are settled |
| `src/task.rs:319` | Free packet-identifier management |
| `src/task.rs:655` | Real error handling instead of `defmt::unwrap!` on `Connect::try_from` |
| `src/task.rs:532` | Support IPv6 in broker DNS resolution (currently `DnsQueryType::A` only) |

The acknowledgement cluster (`:46`, `:72`, `:674`, `:319`) is one refactor, not four. The concurrency
hazard is real but is only exercised at startup with a fixed subscription set today; it becomes
urgent if subscriptions ever become dynamic.

### MQTT packet encode / decode — `src/mqtt/`

| Where | Item |
|---|---|
| `src/mqtt/packet/publish.rs:158` | Validate there is enough space left in the buffer |
| `src/mqtt/packet/publish.rs:146` | Validate the topic name contains no MQTT wildcard characters |
| `src/mqtt/packet/publish.rs:66`, `src/mqtt/mod.rs:54` | Set the duplicate and QoS flags. Retain is done — the reset cause is published with it |
| `src/mqtt/packet/connect.rs:40` | Check the fixed header can even be written |
| `src/mqtt/packet/subscribe.rs:127` | Support packet identifiers greater than `u8::MAX` |
| `src/mqtt/packet/subscribe_acknowledgement.rs:18` | Convert to the decode trait |
| `src/mqtt/packet/subscribe_acknowledgement.rs:27` | Stop ignoring properties |
| `src/mqtt/packet/subscribe_acknowledgement.rs:28` | Check the topics are actually acknowledged |
| `src/mqtt/packet/connect_acknowledgement.rs:187` | Decode properties |
| `src/mqtt/packet/disconnect.rs:103` | Decode more fields when needed |

Of these only `publish.rs:158` has a memory-safety flavour and is worth checking for a panic path;
the rest are protocol conformance polish against a broker you control.

### Modbus — `src/modbus/client.rs`

| Where | Item |
|---|---|
| `src/modbus/client.rs:329` | Understand why the flush must be blocking to avoid `WouldBlock` |

Response validation and the short-read hazard are done; see P0 item 1, reading holding registers is
done; see P1 item 3, and reading input registers is done; see the sensor polling item.

### Configuration — `src/configuration.rs`

`src/configuration.rs:23`, `:27`, `:31`, `:34` all say the same thing: make the Wi-Fi SSID, Wi-Fi
password, broker address, and broker port configurable at runtime instead of baked in by
`build.rs`. This is the same underlying work as the aspirational web-configuration item in
`README.md`, and it is the largest single change on this list — there is no runtime configuration
or persistent storage in the firmware at all today.

### Remaining items from `README.md`

Not already covered above:

- Retry if there was an error writing to one or two fans
- When retrying fails after a while, reset the other fan to avoid under- or overpressure in the house
- Confirm the fan speed is set in Home Assistant and retry otherwise; MQTT QoS can implement this
- Make the button press pick up state changed through Home Assistant instead of keeping its own state
- Switch to only using the refactored `send` for Modbus
- Try bundling all channels into an event-bus / actor-model shape
- Aspirational: a web interface for configuring the fan when Wi-Fi is not set up yet

Already done: debounce the button tap; flash the LEDs for a second after boot.
