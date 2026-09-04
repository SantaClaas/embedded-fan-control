import { For, Show, createMemo, createSignal } from "solid-js";
import {
  devices,
  noContext,
  referencesFor,
  registersIn,
  type Context,
  type Device,
  type Register,
  type Space,
} from "../devices";
import { BAUD_RATE_WRITE, BAUD_RATE_READ, relay } from "../devices/relay";
import { describeFailure, readAll, settingsMismatch, write } from "../serial/client";
import type { Connection } from "../serial/connection";
import { registerAddress, toHex } from "./format";

/**
 * What the last read produced.
 *
 * The values are kept apart by address space, because an address only means something within one:
 * the relay has a register at 0x0000 in three of the four — its address, its relay, its
 * optocoupler input — and one map keyed by address alone had them overwriting each other in read
 * order. What that looked like was a relay that could be closed and then never opened, because the
 * button was reading the optocoupler's state for the coil's
 */
type State = {
  values: ReadonlyMap<Space, ReadonlyMap<number, number>>;
  refused: { space: Space; start: number; quantity: number; message: string }[];
  at?: Date;
};

const spaces = ["holding", "input", "coil", "discreteInput"] as const;

export default function Devices(props: { connection: Connection }) {
  const [device, setDevice] = createSignal<Device>(devices[0]!);
  const [address, setAddress] = createSignal(devices[0]!.defaults.address);
  const [state, setState] = createSignal<State>({ values: new Map(), refused: [] });
  const valuesIn = (space: Space) => state().values.get(space);
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<string | undefined>();

  // Only holding registers are ever referred to by another register's decoder, and only holding
  // values belong in the context — a coil at the same address is a different thing entirely
  const context = createMemo<Context>(() => ({ holding: valuesIn("holding") ?? new Map() }));

  // The port cannot be reconfigured while it is open, and this panel only exists while it is, so
  // reading the connection's settings here is stable for as long as the warning is on screen
  const mismatch = createMemo(() => settingsMismatch(device(), props.connection.settings));

  function chooseDevice(id: string) {
    const chosen = devices.find((entry) => entry.id === id);
    if (!chosen) return;

    setDevice(chosen);
    setAddress(chosen.defaults.address);
    setState({ values: new Map(), refused: [] });
  }

  async function readEverything() {
    setBusy(true);
    setError(undefined);

    try {
      const values = new Map<Space, Map<number, number>>();
      const refused: State["refused"] = [];

      const keep = (space: Space, read: ReadonlyMap<number, number>) => {
        const known = values.get(space) ?? new Map<number, number>();
        for (const [key, value] of read) known.set(key, value);
        values.set(space, known);
      };

      // The references first, and on their own: almost every RadiCal input register is a fraction
      // of one of them, so reading them last would mean a first pass that can state nothing
      const references = referencesFor(device().registers);
      if (references.length > 0) {
        const result = await readAll(
          props.connection,
          address(),
          "holding",
          references.map((reference) => ({ address: reference }) as Register),
        );
        keep("holding", result.values);
      }

      for (const space of spaces) {
        const registers = registersIn(device(), space);
        if (registers.length === 0) continue;

        const result = await readAll(props.connection, address(), space, registers);
        keep(space, result.values);
        refused.push(...result.refused.map((refusal) => ({ ...refusal, space })));
      }

      setState({ values, refused, at: new Date() });
    } catch (cause) {
      setError(describeFailure(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section>
      <header class="panel-header">
        <div>
          <h3>Device</h3>
          <p class="hint">
            Active. This sends requests, so do not use it while the fan controller is driving the
            same bus — two masters on one RS-485 line collide.
          </p>
        </div>
      </header>

      <div class="controls">
        <div class="control">
          <label for="device-type">Device</label>
          <select id="device-type" onChange={(event) => chooseDevice(event.currentTarget.value)}>
            <For each={devices}>{(entry) => <option value={entry.id}>{entry.name}</option>}</For>
          </select>
        </div>

        <div class="control">
          <label for="device-address">Modbus address</label>
          {/* The hint sits above the field, so an autocomplete popover cannot cover it */}
          <span id="device-address-hint" class="hint">
            {device().defaults.addressRange[0]}–{device().defaults.addressRange[1]}, default{" "}
            {device().defaults.address}
          </span>
          <input
            id="device-address"
            type="number"
            min={device().defaults.addressRange[0]}
            max={device().defaults.addressRange[1]}
            step={1}
            required
            value={address()}
            aria-describedby="device-address-hint"
            onInput={(event) => setAddress(event.currentTarget.valueAsNumber)}
          />
        </div>

        <button type="button" onClick={() => void readEverything()} disabled={busy()}>
          {busy() ? "Reading…" : "Read all registers"}
        </button>
      </div>

      <p class="hint">
        Documented in <code>{device().documentation}</code>. Expects {device().defaults.baudRate} baud,{" "}
        {device().defaults.parity} parity — set the port to match before reading.
      </p>

      <Show when={mismatch()}>
        {(message) => (
          <p class="error-msg shown">
            {message()} Close the port and reopen it with the preset for this device.
          </p>
        )}
      </Show>

      <Show when={error()}>{(message) => <p class="error-msg shown">{message()}</p>}</Show>

      <Show when={state().at}>
        {(at) => <p class="hint">Last read {at().toLocaleTimeString()}</p>}
      </Show>

      <For each={state().refused}>
        {(refusal) => (
          <p class="error-msg shown">
            {spaceName[refusal.space]} {registerAddress(refusal.start)} +{refusal.quantity}:{" "}
            {refusal.message}
          </p>
        )}
      </For>

      <For each={["input", "holding", "coil", "discreteInput"] as const}>
        {(space) => (
          <Show when={registersIn(device(), space).length > 0}>
            <h4>{spaceHeading[space]}</h4>
            <div class="table-scroll">
              <table class="registers">
                <thead>
                  <tr>
                    <th>Address</th>
                    <th>Name</th>
                    <th>Value</th>
                    <th>Write</th>
                  </tr>
                </thead>
                <tbody>
                  <For each={registersIn(device(), space)}>
                    {(register) => (
                      <RegisterRow
                        connection={props.connection}
                        deviceAddress={address()}
                        register={register}
                        raw={valuesIn(register.space)?.get(register.address)}
                        context={context()}
                        onWritten={() => void readEverything()}
                      />
                    )}
                  </For>
                </tbody>
              </table>
            </div>
          </Show>
        )}
      </For>
    </section>
  );
}

const spaceHeading = {
  input: "Input registers — read only",
  holding: "Holding registers — read and write",
  coil: "Coils — read and write",
  discreteInput: "Discrete inputs — read only",
} as const;

/** Short enough to put in front of an address, which does not identify a read on its own */
const spaceName = {
  input: "Input register",
  holding: "Holding register",
  coil: "Coil",
  discreteInput: "Discrete input",
} as const;

function RegisterRow(props: {
  connection: Connection;
  deviceAddress: number;
  register: Register;
  raw: number | undefined;
  context: Context;
  onWritten: () => void;
}) {
  const [pending, setPending] = createSignal<number | undefined>();
  const [failure, setFailure] = createSignal<string | undefined>();
  const [writing, setWriting] = createSignal(false);
  // The browser already knows whether the value is within the min/max/step the register declares,
  // so its judgement is the one used rather than a second copy of the bounds check here
  const [valid, setValid] = createSignal(false);
  let field: HTMLInputElement | undefined;

  const decoded = createMemo(() => {
    const raw = props.raw;
    return raw === undefined ? undefined : props.register.decode(raw, props.context ?? noContext);
  });

  async function send() {
    const value = pending();
    const writable = props.register.write;
    if (value === undefined || !writable) return;

    // Refuse to put a value on the bus that the device is bound to reject anyway
    if (field && !field.validity.valid) {
      field.reportValidity();
      return;
    }

    setWriting(true);
    setFailure(undefined);

    try {
      await write(props.connection, props.deviceAddress, props.register, writable.encode(value), {
        // The relay's manual only ever writes its holding registers with 0x10, and reads its baud
        // rate from a different address than it writes it to
        useMultiple: props.register.space === "holding" && isRelayRegister(props.register),
        writeAddress: props.register.address === BAUD_RATE_READ && isRelayRegister(props.register) ? BAUD_RATE_WRITE : undefined,
      });

      props.onWritten();
    } catch (cause) {
      setFailure(describeFailure(cause));
    } finally {
      setWriting(false);
    }
  }

  const inputId = () => `write-${props.register.space}-${props.register.address}`;

  return (
    <tr>
      <td class="address">
        {registerAddress(props.register.address)}
        <span class="hint"> §{props.register.reference.replace(/^docs\/.*, /, "")}</span>
      </td>
      <td>
        <span class="name">{props.register.name}</span>
        <Show when={props.register.gloss}>{(gloss) => <span class="hint"> — {gloss()}</span>}</Show>
      </td>
      <td>
        <Show when={decoded()} fallback={<span class="hint">not read</span>}>
          {(value) => (
            <>
              <span class={value().kind === "unavailable" ? "hint" : "value"}>{value().text}</span>
              <Show when={value().kind === "unavailable" ? value() : undefined}>
                {(reason) => <span class="hint"> — {(reason() as { because: string }).because}</span>}
              </Show>
              <Show when={props.raw !== undefined}>
                <span class="hint"> raw 0x{toHex(props.raw!, 4)}</span>
              </Show>
            </>
          )}
        </Show>
      </td>
      <td>
        <Show when={props.register.write} fallback={<span class="hint">—</span>}>
          {(writable) => (
            <div class="field">
              <Show
                when={writable().input.control === "choice" ? writable().input : undefined}
                fallback={
                  <Show
                    when={writable().input.control === "number" ? writable().input : undefined}
                    fallback={
                      <button type="button" disabled={writing()} onClick={() => { setPending(props.raw ? 0 : 1); void send(); }}>
                        {props.raw ? "Open" : "Close"}
                      </button>
                    }
                  >
                    {(numeric) => (
                      <>
                        <input
                          ref={field}
                          id={inputId()}
                          type="number"
                          required
                          min={(numeric() as { min: number }).min}
                          max={(numeric() as { max: number }).max}
                          step={(numeric() as { step?: number }).step ?? 1}
                          aria-errormessage={`${inputId()}-error`}
                          aria-invalid={pending() !== undefined && !valid() ? "true" : undefined}
                          onInput={(event) => {
                            setPending(event.currentTarget.valueAsNumber);
                            setValid(event.currentTarget.validity.valid);
                          }}
                        />
                        {/* Revealed by :user-invalid, so nothing is flagged until the field is left */}
                        <span id={`${inputId()}-error`} class="error-msg">
                          {(numeric() as { min: number }).min} to {(numeric() as { max: number }).max}
                        </span>
                      </>
                    )}
                  </Show>
                }
              >
                {(choice) => (
                  <select
                    onChange={(event) => {
                      setPending(Number(event.currentTarget.value));
                      setValid(event.currentTarget.value !== "");
                    }}
                  >
                    <option value="">Choose…</option>
                    <For each={[...(choice() as { options: ReadonlyMap<number, string> }).options]}>
                      {([value, label]) => <option value={value}>{label}</option>}
                    </For>
                  </select>
                )}
              </Show>

              <Show when={writable().input.control !== "toggle"}>
                <button
                  type="button"
                  disabled={writing() || pending() === undefined || (writable().input.control === "number" && !valid())}
                  onClick={() => void send()}
                >
                  {writing() ? "Writing…" : "Write"}
                </button>
              </Show>

              <Show when={failure()}>{(message) => <span class="error-msg shown">{message()}</span>}</Show>
            </div>
          )}
        </Show>
      </td>
    </tr>
  );
}

function isRelayRegister(register: Register): boolean {
  return relay.registers.includes(register);
}
