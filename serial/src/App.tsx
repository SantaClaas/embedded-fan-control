import { Show } from "solid-js";

/**
 * Whether this browser can talk to a serial port at all.
 *
 * Read once rather than reactively: the Web Serial API either is or is not in the browser, and it
 * does not appear part-way through a session. It is also gated on a secure context, so a page
 * served over plain HTTP from anything but localhost lands here too
 */
const isSupported = "serial" in navigator;

export default function App() {
  return (
    <main>
      <h1>Modbus Serial Tool</h1>

      <Show when={isSupported} fallback={<Unsupported />}>
        <p>Web Serial is available.</p>
      </Show>
    </main>
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
