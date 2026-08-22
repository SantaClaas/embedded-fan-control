# TODO inventory

Every outstanding item in the firmware, collected from [README.md](README.md),
[documentation.md](documentation.md), and `//TODO` comments in `src/`.
Paths and line numbers are relative to `fan-controller/`. They drift as the source changes, so
treat them as a starting point rather than an exact address.

The first section is a suggested order of work with the reasoning; the sections after it are the
full inventory grouped by area, so nothing gets lost.

---

## Suggested priority

### P0 — the two items that make the device wrong or dead in the field

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
checksum checked and returned as `Error::Exception`, so the existing retry loop now fires for a fan
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

### P1 — state is wrong after every reset

**3. Read the fan speed on boot** — done, `src/modbus/`, `src/main.rs`

After a reset the fans keep spinning at whatever they were set to, but the controller assumed
nothing: `current_set_point` started as `None` and `last_fan_state` at `SetPoint::ZERO`. The LEDs
and the Home Assistant state were wrong until someone issued a command, and a Home Assistant "on"
restored a set point that was never actually in effect.

Reading holding registers (function code `0x03`) is now implemented. `ReadHoldingRegister` asks for
exactly one register, which keeps the response a fixed length, and `Client::read_holding_register`
returns its contents. The parts both functions share are factored out of the write path rather than
copied: `send_request` drives the line and writes the frame, and `read_header` reads the address and
function code, turns an exception frame into `Error::Exception`, and returns once the header is the
answer that was asked for, so each transaction only has to read its own body. A read is rejected if
it announces a byte count other than the one register asked for, which is checked before the
checksum because a different length means the wrong bytes were just read. `Exception` covers both
functions now: `0x03` (response too long) only exists for reads, and `WriteRefused` became
`AccessRefused` because `0x04` also means a register that cannot be read. `send_3` was renamed to
`write_holding_register` to pair with it.

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

**4. Fans ping-pong forever when the bus is down** — `src/main.rs:849`

After exhausting `MAX_ATTEMPTS`, a fan signals the *other* fan back to its own last known good set
point. If the bus itself is down, that fan fails too and signals back, and neither ever stops.

It is a slow churn rather than a spin — roughly four attempts at a 5 s timeout plus backoff per
round — but it never terminates and keeps cycling the Modbus mutex. It used to be impossible on a
cold boot, because `current_set_point` stayed `None` until the first successful write and
`Option::inspect` then does nothing. P1 item 3 removed that accident: the boot read can fill
`current_set_point` before any write has succeeded, so a bus that dies right after boot now reaches
this too. The fix options are already written in place at `src/main.rs:850` and `src/main.rs:851` —
a once-only retry strategy carried on the signal, or a counter that detects the ping-pong.

### Cheap win worth slotting in anywhere

**Make `SetPoint` host-testable** — `src/fan/set_point.rs:67`

The `#[cfg(test)]` module never compiles, and `CLAUDE.md` records that it has already rotted once
and is kept correct by hand. Extracting `SetPoint` into its own crate is a small, self-contained
change that turns the only meaningful unit tests in the firmware back on.

---

## Full inventory

### Fan state and control logic — `src/main.rs`

| Where | Item |
|---|---|
| `src/main.rs:633` | Make `is_synchronization_on` configurable through a switch (hardcoded `true`) |
| `src/main.rs:847` | Skip pushing the other fan's speed when a setting allows the fans to run out of sync |
| `src/main.rs:849` | Fix the endless loop when both fans fail and keep signalling each other back |
| `src/main.rs:850` | Option A: a retry strategy on the signal, set to once |
| `src/main.rs:851` | Option B: a counter that detects the loop |
| `src/main.rs:794` | Consider updating the display state even when the new set point equals the current one |
| `src/main.rs:262`, `:275`, `:300`, `:313` | Handle backpressure when the MQTT out channel is full |

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
| `src/mqtt/packet/publish.rs:66`, `src/mqtt/mod.rs:54` | Set flags |
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

Response validation and the short-read hazard are done; see P0 item 1, and reading holding
registers is done; see P1 item 3.

### Configuration — `src/configuration.rs`

`src/configuration.rs:23`, `:27`, `:31`, `:34` all say the same thing: make the Wi-Fi SSID, Wi-Fi
password, broker address, and broker port configurable at runtime instead of baked in by
`build.rs`. This is the same underlying work as the aspirational web-configuration item in
`README.md`, and it is the largest single change on this list — there is no runtime configuration
or persistent storage in the firmware at all today.

### Testing and documentation

| Where | Item |
|---|---|
| `src/fan/set_point.rs:67` | Move `SetPoint` into a host-testable crate so its tests actually run |
| `documentation.md:53` | Write the Wiring section: Pico W, debug probe, MAX485, status LEDs, button |

### Remaining items from `README.md`

Not already covered above:

- Retry if there was an error writing to one or two fans
- When retrying fails after a while, reset the other fan to avoid under- or overpressure in the house
- Confirm the fan speed is set in Home Assistant and retry otherwise; MQTT QoS can implement this
- Make the button press pick up state changed through Home Assistant instead of keeping its own state
- Read temperature sensors
- Switch to only using the refactored `send` for Modbus
- Try bundling all channels into an event-bus / actor-model shape
- Aspirational: a web interface for configuring the fan when Wi-Fi is not set up yet

Already done: debounce the button tap; flash the LEDs for a second after boot.
