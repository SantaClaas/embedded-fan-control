import { For, Show, createSignal, onCleanup } from "solid-js";
import PortPanel from "./ui/PortPanel";

/**
 * Whether this browser can talk to a serial port at all.
 *
 * Read once rather than reactively: the Web Serial API either is or is not in the browser, and it
 * does not appear part-way through a session. It is also gated on a secure context, so a page
 * served over plain HTTP from anything but localhost lands here too
 */
const isSupported = "serial" in navigator;

type Entry = { port: SerialPort; info: Partial<SerialPortInfo> };

export default function App() {
  return (
    <main>
      <header class="masthead">
        <h1>Modbus Serial Tool</h1>
        <p class="hint">
          Watches the RS-485 bus and reads or writes the registers of the devices on it. Register
          names and codings come from the manufacturers' own documentation.
        </p>
      </header>

      <Show when={isSupported} fallback={<Unsupported />}>
        <Ports />
      </Show>
    </main>
  );
}

function Ports() {
  const [ports, setPorts] = createSignal<readonly Entry[]>([]);
  const [error, setError] = createSignal<string | undefined>();

  function remember(port: SerialPort) {
    setPorts((current) =>
      current.some((entry) => entry.port === port) ? current : [...current, { port, info: port.getInfo() }],
    );
  }

  function forget(port: SerialPort) {
    setPorts((current) => current.filter((entry) => entry.port !== port));
  }

  // Solid 2.0 has no onMount; a component body runs once during setup, which is what onMount
  // existed to arrange, and onCleanup is tied to the owner rather than to mounting
  //
  // Ports the user has already granted this origin access to, which come back without a prompt
  void navigator.serial.getPorts().then((granted) => granted.forEach(remember));

  const onConnect = (event: Event) => remember(event.target as SerialPort);
  const onDisconnect = (event: Event) => forget(event.target as SerialPort);

  navigator.serial.addEventListener("connect", onConnect);
  navigator.serial.addEventListener("disconnect", onDisconnect);

  onCleanup(() => {
    navigator.serial.removeEventListener("connect", onConnect);
    navigator.serial.removeEventListener("disconnect", onDisconnect);
  });

  async function add() {
    setError(undefined);

    try {
      // No filter: the old tool hardcoded the CH340's USB identifiers, which hid every other
      // adapter from the picker. The user knows which one they plugged in
      remember(await navigator.serial.requestPort());
    } catch (cause) {
      // Dismissing the picker rejects, and is not an error worth showing
      if (cause instanceof DOMException && cause.name === "NotFoundError") return;
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  return (
    <>
      <div class="controls">
        <button type="button" onClick={() => void add()}>
          Add a port…
        </button>
      </div>

      <Show when={error()}>{(message) => <p class="error-msg shown">{message()}</p>}</Show>

      <Show
        when={ports().length > 0}
        fallback={
          <p class="empty">
            No ports yet. Choose <strong>Add a port…</strong> and pick the USB serial adapter
            connected to the RS-485 bus.
          </p>
        }
      >
        <For each={ports()}>{(entry) => <PortPanel port={entry.port} info={entry.info} />}</For>
      </Show>
    </>
  );
}

function Unsupported() {
  return (
    <section class="notice">
      <h2>This browser cannot open a serial port</h2>
      <p>
        The tool needs the <a href="https://developer.mozilla.org/en-US/docs/Web/API/Web_Serial_API">Web Serial API</a>,
        which is only in Chromium-based browsers — Chrome, Edge and Opera — and only on a secure origin.
      </p>
      <p>
        If you are on one of those and still see this, check that the page is served over HTTPS or from{" "}
        <code>localhost</code>.
      </p>
    </section>
  );
}
