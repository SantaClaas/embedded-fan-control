import { For, Show, createSignal } from "solid-js";
import { Connection, baudRates, defaultSettings, type Parity, type PortSettings } from "../serial/connection";
import Devices from "./Devices";
import Monitor from "./Monitor";

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

type Mode = "monitor" | "devices";

export default function PortPanel(props: { port: SerialPort; info: Partial<SerialPortInfo> }) {
  const connection = new Connection(props.port);

  const [settings, setSettings] = createSignal<PortSettings>(defaultSettings);
  const [isOpen, setIsOpen] = createSignal(false);
  const [mode, setMode] = createSignal<Mode>("monitor");
  const [error, setError] = createSignal<string | undefined>();

  async function toggle() {
    setError(undefined);

    try {
      if (isOpen()) {
        await connection.close();
        setIsOpen(false);
        return;
      }

      await connection.open(settings());
      setIsOpen(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  function update<K extends keyof PortSettings>(key: K, value: PortSettings[K]) {
    setSettings((current) => ({ ...current, [key]: value }));
  }

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

        <button type="button" onClick={() => void toggle()}>
          {isOpen() ? "Close port" : "Open port"}
        </button>
      </header>

      <Show when={error()}>{(message) => <p class="error-msg shown">{message()}</p>}</Show>

      <fieldset class="controls" disabled={isOpen()}>
        <legend>Port settings</legend>

        <div class="control">
          <label for="preset">Preset</label>
          <select
            id="preset"
            onChange={(event) => {
              const preset = presets[Number(event.currentTarget.value)];
              if (preset) setSettings(preset.settings);
            }}
          >
            <For each={presets}>{(preset, index) => <option value={index()}>{preset.label}</option>}</For>
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
          <button type="button" aria-pressed={mode() === "monitor" ? "true" : "false"} onClick={() => setMode("monitor")}>
            Watch the bus
          </button>
          <button type="button" aria-pressed={mode() === "devices" ? "true" : "false"} onClick={() => setMode("devices")}>
            Talk to a device
          </button>
        </nav>

        <Show when={mode() === "monitor"} fallback={<Devices connection={connection} />}>
          <Monitor connection={connection} />
        </Show>
      </Show>
    </article>
  );
}
