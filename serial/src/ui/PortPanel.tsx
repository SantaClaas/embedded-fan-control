import { For, Show, createEffect, createSignal } from "solid-js";
import { Connection, baudRates, defaultSettings, type Parity, type PortSettings } from "../serial/connection";
import Devices from "./Devices";
import Monitor from "./Monitor";
import { load, sameSettings, save, type Mode, type Remembered } from "./preferences";

/**
 * Settings that make a device answer at all.
 *
 * The RadiCal manual recommends 19200 8E1 and `fan-controller` is built for it; both the relay and
 * the temperature sensor default to 9600 with no parity. A port speaks one of these at a time, so
 * a bus carrying both kinds of device has to be visited twice
 */
const presets: readonly { label: string; settings: PortSettings }[] = [
  { label: "RadiCal fans — 19200 8E1", settings: defaultSettings },
  {
    label: "Relay / temperature sensor — 9600 8N1",
    settings: { baudRate: 9600, parity: "none", dataBits: 8, stopBits: 1 },
  },
];

export default function PortPanel(props: {
  port: SerialPort;
  info: Partial<SerialPortInfo>;
  /** Where this port's remembered choices are kept, from `portKey` */
  storageKey: string;
}) {
  const connection = new Connection(props.port);

  // Every choice this panel offers, as it was left last time. One record rather than a signal
  // each, because they are saved and restored together and a panel that remembered half of them
  // would be worse than one that remembered none
  const [remembered, setRemembered] = createSignal<Remembered>(load(props.storageKey));
  const settings = () => remembered().settings;
  const mode = () => remembered().mode;

  function remember(change: Partial<Remembered>) {
    setRemembered((current) => ({ ...current, ...change }));
  }

  // The compute tracks the record, the effect writes it out. Saving here rather than in each
  // handler means there is one place a change can fail to be remembered from, instead of six
  createEffect(
    () => remembered(),
    (value) => save(props.storageKey, value),
  );

  const [isOpen, setIsOpen] = createSignal(false);
  const [error, setError] = createSignal<string | undefined>();

  async function open() {
    setError(undefined);

    try {
      await connection.open(settings());
      setIsOpen(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  async function toggle() {
    if (!isOpen()) return open();

    setError(undefined);

    try {
      await connection.close();
      setIsOpen(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  // A component body runs once during setup, so this is the port being opened as the panel
  // appears — on a reload, and equally when the adapter is plugged back in. Only ever for a port
  // that has been asked to do it; a failure lands in the same place a failed click would
  if (remembered().reopen) void open();

  function update<K extends keyof PortSettings>(key: K, value: PortSettings[K]) {
    remember({ settings: { ...settings(), [key]: value } });
  }

  /**
   * Which preset the current settings are, if they are one.
   *
   * Restored settings have to put the select back where the user left it, and a select showing
   * "RadiCal fans" over a port about to be opened at 9600 8N1 is worse than no preset at all
   */
  const preset = () => presets.findIndex((entry) => sameSettings(entry.settings, settings()));

  return (
    <article class="port">
      <header class="panel-header">
        <div>
          <h2>
            Serial port
            <Show when={props.info.usbVendorId !== undefined}>
              <span class="hint">
                {" "}
                USB {props.info.usbVendorId?.toString(16)}:{props.info.usbProductId?.toString(16)}
              </span>
            </Show>
          </h2>
        </div>

        <div class="actions">
          {/* Outside the settings fieldset, which is disabled while the port is open: this is a
              choice about the next visit rather than about this connection */}
          <label class="checkbox">
            <input
              type="checkbox"
              checked={remembered().reopen}
              onChange={(event) => remember({ reopen: event.currentTarget.checked })}
            />
            Open automatically
          </label>

          <button type="button" onClick={() => void toggle()}>
            {isOpen() ? "Close port" : "Open port"}
          </button>
        </div>
      </header>

      <Show when={error()}>{(message) => <p class="error-msg shown">{message()}</p>}</Show>

      <fieldset class="controls" disabled={isOpen()}>
        <legend>Port settings</legend>

        <div class="control">
          <label for="preset">Preset</label>
          <select
            id="preset"
            value={preset()}
            onChange={(event) => {
              const chosen = presets[Number(event.currentTarget.value)];
              if (chosen) remember({ settings: chosen.settings });
            }}
          >
            {/* Only while the settings are nobody's preset, so it cannot be chosen — there is
                nothing for choosing "Custom" to do */}
            <Show when={preset() < 0}>
              <option value={-1}>Custom</option>
            </Show>
            <For each={presets}>{(entry, index) => <option value={index()}>{entry.label}</option>}</For>
          </select>
        </div>

        <div class="control">
          <label for="baud">Baud rate</label>
          <select id="baud" value={settings().baudRate} onChange={(event) => update("baudRate", Number(event.currentTarget.value))}>
            <For each={baudRates}>{(rate) => <option value={rate}>{rate}</option>}</For>
          </select>
        </div>

        <div class="control">
          <label for="parity">Parity</label>
          <select id="parity" value={settings().parity} onChange={(event) => update("parity", event.currentTarget.value as Parity)}>
            <For each={["none", "even", "odd"] as const}>{(parity) => <option value={parity}>{parity}</option>}</For>
          </select>
        </div>

        <div class="control">
          <label for="stop-bits">Stop bits</label>
          <select id="stop-bits" value={settings().stopBits} onChange={(event) => update("stopBits", Number(event.currentTarget.value) as 1 | 2)}>
            <option value={1}>1</option>
            <option value={2}>2</option>
          </select>
        </div>
      </fieldset>

      <Show
        when={isOpen()}
        fallback={<p class="empty">Open the port to watch the bus or read a device.</p>}
      >
        <nav class="tabs" aria-label="What to do with this port">
          <For each={["monitor", "devices"] as const}>
            {(tab) => (
              <button
                type="button"
                aria-pressed={mode() === tab ? "true" : "false"}
                onClick={() => remember({ mode: tab })}
              >
                {tabLabel[tab]}
              </button>
            )}
          </For>
        </nav>

        <Show
          when={mode() === "monitor"}
          fallback={
            <Devices
              connection={connection}
              device={remembered().device}
              addresses={remembered().addresses}
              onChoose={(device, addresses) => remember({ device, addresses })}
            />
          }
        >
          <Monitor connection={connection} />
        </Show>
      </Show>
    </article>
  );
}

const tabLabel: Record<Mode, string> = {
  monitor: "Watch the bus",
  devices: "Talk to a device",
};
