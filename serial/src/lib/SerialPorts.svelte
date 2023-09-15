<script lang="ts">
  import { onMount } from "svelte";
  import SerialPortView from "./SerialPortSettings.svelte";

  function isSerialPort(object: EventTarget | null): object is SerialPort {
    return object !== null && "onconnect" in object;
  }

  let availablePorts: Map<SerialPort, Partial<SerialPortInfo>> = new Map();

  function addPort(event: Event) {
    if (!isSerialPort(event.target)) return;

    availablePorts.set(event.target, event.target.getInfo());
  }

  function removePort(event: Event) {
    if (!isSerialPort(event.target)) return;

    availablePorts.delete(event.target);
  }

  onMount(() => {
    navigator.serial.addEventListener("connect", addPort);

    navigator.serial.addEventListener("disconnect", removePort);

    return () => {
      navigator.serial.removeEventListener("connect", addPort);
      navigator.serial.removeEventListener("disconnect", removePort);
    };
  });

  navigator.serial.getPorts().then((ports) => {
    availablePorts = new Map(ports.map((port) => [port, port.getInfo()]));
  });

  // Known working usb devices
  const portFilter = [{ usbVendorId: 6790, usbProductId: 29987 }];
  async function connect() {
    let port;
    try {
      port = await navigator.serial.requestPort({ filters: portFilter });
    } catch (error) {
      console.warn("User cancelled port selection", error);
      return;
    }

    availablePorts.set(port, port.getInfo());
    quickUpdateHack();
  }

  function quickUpdateHack() {
    // TODO remove this hack to force rerender
    availablePorts = availablePorts;
  }
</script>

<main>
  <h2>Serial Ports</h2>
  <ul>
    {#each availablePorts as [port, info]}
      {#key port}
        <li>
          <SerialPortView {port} {info} />
        </li>
      {/key}
    {/each}
  </ul>

  <button on:click={connect}>Add Port</button>
</main>

<style>
  ul {
    all: unset;
    list-style-type: none;
  }
</style>
