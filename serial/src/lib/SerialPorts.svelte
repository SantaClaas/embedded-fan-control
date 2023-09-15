<script lang="ts">
  import { onMount } from "svelte";
  import SerialPortView from "./SerialPortSettings.svelte";

  function isSerialPort(object: EventTarget | null): object is SerialPort {
    return object !== null && "onconnect" in object;
  }

  let availablePorts: Map<SerialPort, Partial<SerialPortInfo>> = new Map();

  onMount(() => {
    navigator.serial.addEventListener("connect", (event) => {
      if (!isSerialPort(event.target)) return;

      console.log("connected", event.target, typeof event.target);
      availablePorts.set(event.target, event.target.getInfo());
    });

    navigator.serial.addEventListener("disconnect", (event) => {
      if (!isSerialPort(event.target)) return;

      console.log("disconnected", event.target);
      availablePorts.delete(event.target);
    });
    console.log("Event listener added");
  });

  navigator.serial.getPorts().then((ports) => {
    availablePorts = new Map(ports.map((port) => [port, port.getInfo()]));
    ports.forEach((port) => console.info("Port:", port.getInfo()));
  });

  // Known working usb devices
  const portFilter = [{ usbVendorId: 6790, usbProductId: 29987 }];
  function connect() {
    navigator.serial.requestPort({ filters: portFilter }).then((port) => {
      availablePorts.set(port, port.getInfo());
      quickUpdateHack();
    });
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
