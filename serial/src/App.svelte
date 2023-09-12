<script lang="ts">
  import { onMount } from "svelte";

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

  function connect() {
    navigator.serial
      .requestPort()
      .then((event) => console.log("port requested"));
  }

  function quickUpdateHack() {
    // TODO remove this hack to force rerender
    availablePorts = availablePorts;
  }

  function open(port: SerialPort) {
    return async () => {
      console.log("Opening port", port);
      await port.open({ baudRate: 9600 });
      quickUpdateHack();
      console.log("Port opened", port.getInfo());
    };
  }

  function close(port: SerialPort) {
    return async () => {
      await port.close();
      quickUpdateHack();
    };
  }

  const data = new Uint8Array([104, 101, 108, 108, 111]); // hello
  function write(port: SerialPort) {
    return async () => {
      console.log("Writing port");
      const writer = port.writable.getWriter();
      await writer.write(data);
      writer.releaseLock();
      console.log("Wrote to port");
    };
  }
</script>

<main>
  <h2>Serial Ports:</h2>
  <ul>
    {#each availablePorts as [port, info]}
      <li>
        <p>USB Product Id: {info.usbProductId}</p>
        <p>Vendor Id: {info.usbVendorId}</p>

        {#if port.writable || port.readable}
          {#if port.writable}
            <button on:click={write(port)}>Write</button>
          {/if}
          <button on:click={close(port)}>Close</button>
        {:else}
          <button on:click={open(port)}>Open</button>
        {/if}
      </li>
    {/each}
  </ul>

  <button on:click={connect}>Connect</button>
</main>

<style>
</style>
