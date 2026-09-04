import { For, Show, createSignal, onCleanup } from "solid-js";
import PortPanel from "./ui/PortPanel";
import { portKey } from "./ui/preferences";

/**
 * Whether this browser can talk to a serial port at all.
 *
 * Read once rather than reactively: the Web Serial API either is or is not in the browser, and it
 * does not appear part-way through a session. It is also gated on a secure context, so a page
 * served over plain HTTP from anything but localhost lands here too
 */
const isSupported = "serial" in navigator;

/**
 * A port as this page sees it: the port itself, what the browser can say about the hardware, and
 * the name its remembered choices are filed under — assigned here, because it is only among the
 * other ports that a second identical adapter can be told from the first
 */
type Entry = { port: SerialPort; info: Partial<SerialPortInfo>; key: string };

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

      <Version />
    </main>
  );
}

/**
 * What this page was built from, stamped in by `define` in vite.config.ts. Read into a constant
 * here because every mention of `__BUILD__` is replaced by the whole literal
 */
const build = __BUILD__;

/**
 * Which build this is, so a page open on a bench can be told apart from the one that was just
 * deployed. The hash links to the repository as it stood at that commit.
 *
 * Nothing here is reactive; Show is only how the missing case is written
 */
function Version() {
  return (
    <footer class="version">
      {build.version}
      <Show when={build.commit}>
        {(commit) => (
          <>
            +
            {/* In a new tab: leaving this one closes the port with it, losing the monitor's
                frames and the connection the user is in the middle of using */}
            <a href={commit().url} target="_blank" rel="noreferrer">
              {commit().short}
            </a>
          </>
        )}
      </Show>
    </footer>
  );
}

function Ports() {
  const [ports, setPorts] = createSignal<readonly Entry[]>([]);
  const [error, setError] = createSignal<string | undefined>();

  function remember(port: SerialPort) {
    setPorts((current) => {
      if (current.some((entry) => entry.port === port)) return current;

      const info = port.getInfo();
      return [...current, { port, info, key: portKey(info, current.map((entry) => entry.key)) }];
    });
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
        <For each={ports()}>
          {(entry) => <PortPanel port={entry.port} info={entry.info} storageKey={entry.key} />}
        </For>
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
