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
    navigator.serial.requestPort().then((port) => {
      availablePorts.set(port, port.getInfo());
      quickUpdateHack();
    });
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

  function readTemperature(port: SerialPort) {
    return async () => {
      const writer = port.writable.getWriter();

      const slaveAddress = 0x01,
        functionCode = 0x04,
        registerAddressHighByte = 0x00,
        registerAddressLowByte = 0x01,
        numberOfRegistersHighByte = 0x00,
        numberOfRegistersLowByte = 0x01,
        crcHighByte = 0x60,
        crcLowByte = 0x0a;

      const command = new Uint8Array([
        slaveAddress,
        functionCode,
        registerAddressHighByte,
        registerAddressLowByte,
        numberOfRegistersHighByte,
        numberOfRegistersLowByte,
        crcHighByte,
        crcLowByte,
      ]);

      await writer.write(command);
      writer.releaseLock();

      if (!port.readable) return;

      const reader = port.readable.getReader();

      try {
        const { value, done } = await reader.read();

        if (done) {
          reader.releaseLock();
          console.log("Done reading");
          return;
        }

        if (!value) return;
        console.log("Read value: ", value);

        if (!(value instanceof Uint8Array)) return;

        // Interpret the binary data
        const view = new DataView(value.buffer);

        const temperatureValue = view.getInt16(3);
        // e.g. 305 = 30.5 ℃
        const temperature = temperatureValue / 10;
        console.log(`Temperature read: ${temperature}℃`);

        reader.releaseLock();
      } catch (error) {
        console.warn("Non-fatal read error:", error);
        reader.releaseLock();
      }
    };
  }
</script>

<main>
  <h2>Serial Ports:</h2>
  <ul>
    {#each availablePorts as [port, info]}
      <li>
        <p>Product Id: {info.usbProductId}</p>
        <p>Vendor Id: {info.usbVendorId}</p>

        {#if port.writable || port.readable}
          {#if port.writable}
            <button on:click={write(port)}>Write</button>
          {/if}

          {#if port.writable && port.readable}
            <button on:click={readTemperature(port)}>Read Temperature</button>
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
