import { For, Show, createEffect, createSignal, onCleanup } from "solid-js";
import { BusMonitor, type MonitorEvent, type Observed } from "../modbus/monitor";
import type { Connection } from "../serial/connection";
import { describe, elapsed, frameToHex, registerAddress, toHex } from "./format";

/**
 * How many events are kept. A busy bus produces a few hundred frames a minute, and the interesting
 * part is nearly always the recent end, so the list is capped rather than grown without limit
 */
const KEEP = 500;

export default function Monitor(props: { connection: Connection }) {
  const [events, setEvents] = createSignal<readonly MonitorEvent[]>([]);
  const [paused, setPaused] = createSignal(false);
  const [origin, setOrigin] = createSignal<number | undefined>();

  // Solid 2.0 splits an effect into a compute and an effect function: the first tracks, the second
  // acts on what it produced. Here the tracked value is the connection, and re-subscribing when it
  // changes is the side effect — the returned function tears the old subscription down
  createEffect(
    () => props.connection,
    (connection) => {
      const monitor = new BusMonitor();

      const unsubscribe = connection.subscribe(({ bytes, at }) => {
        if (paused()) return;

        const produced = monitor.push(bytes, at);
        if (produced.length === 0) return;

        setOrigin((current) => current ?? produced[0]!.at);
        setEvents((current) => [...current, ...produced].slice(-KEEP));
      });

      onCleanup(unsubscribe);
    },
  );

  return (
    <section>
      <header class="panel-header">
        <div>
          <h3>Bus monitor</h3>
          <p class="hint">
            Passive. Nothing is written to the port, so this is safe to leave running while the fan
            controller drives the bus.
          </p>
        </div>

        <div class="actions">
          <button type="button" onClick={() => setPaused((value) => !value)}>
            {paused() ? "Resume" : "Pause"}
          </button>
          <button type="button" onClick={() => setEvents([])}>
            Clear
          </button>
        </div>
      </header>

      <Show
        when={events().length > 0}
        fallback={
          <p class="empty">
            Nothing on the bus yet. Frames appear here as soon as something transmits — check the
            baud rate and parity if the controller is running and this stays empty.
          </p>
        }
      >
        <div class="table-scroll">
          <table class="frames">
            <thead>
              <tr>
                <th>Time</th>
                <th>From</th>
                <th>Address</th>
                <th>Function</th>
                <th>Meaning</th>
                <th>Bytes</th>
              </tr>
            </thead>
            <tbody>
              <For each={events()}>{(event) => <Row event={event} origin={origin() ?? 0} />}</For>
            </tbody>
          </table>
        </div>
      </Show>
    </section>
  );
}

function Row(props: { event: MonitorEvent; origin: number }) {
  return (
    <Show
      when={props.event.type === "frame" ? props.event : undefined}
      fallback={
        <tr class="row noise">
          <td>{elapsed(props.event.at, props.origin)}</td>
          <td colspan={4}>Unaccounted bytes — the monitor could not fit these into a frame</td>
          <td class="bytes">{frameToHex(props.event.bytes)}</td>
        </tr>
      }
    >
      {(frame) => (
        <tr class={`row ${frame().role}`}>
          <td>{elapsed(frame().at, props.origin)}</td>
          <td>{frame().role === "request" ? "Master" : "Device"}</td>
          <td>{frame().address}</td>
          <td>0x{toHex(frame().functionCode)}</td>
          <td>
            {describe(frame().decoded)}
            {/*
              A read response carries values but not the addresses they came from. Where the
              monitor managed to pair it with its request, say which range they belong to
            */}
            <Show when={answeredRange(frame().inReplyTo?.decoded)}>
              {(start) => <span class="hint"> — from {registerAddress(start())}</span>}
            </Show>
          </td>
          <td class="bytes">{frameToHex(frame().bytes)}</td>
        </tr>
      )}
    </Show>
  );
}

/**
 * The start address a paired request was asking about, when it was a read.
 *
 * Narrowing here rather than inline, because `decoded` is a union in which only some members have
 * a `start` at all and TypeScript cannot follow that through a call inside a ternary
 */
function answeredRange(decoded: Observed["decoded"] | undefined): number | undefined {
  if (!decoded) return undefined;

  return decoded.kind === "read" ? decoded.start : undefined;
}
