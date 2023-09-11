<script lang="ts">
  console.log("start");

  function isSerialPort(object: EventTarget | null): object is SerialPort {
    return object !== null && "onconnect" in object;
  }

  let availablePorts: Map<SerialPort, Partial<SerialPortInfo>> = new Map();
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

  navigator.serial.getPorts().then((ports) => {
    availablePorts = new Map(ports.map((port) => [port, port.getInfo()]));
    ports.forEach((port) => console.info("Port:", port.getInfo()));
  });

  function connect() {
    navigator.serial
      .requestPort()
      .then((event) => console.log("port requested"));
  }

  function send(port: SerialPort) {
    return async () => {
      console.log("Send on port", port);
      await port.open({ baudRate: 9600 });
    };
  }
</script>

<main>
  <h2>Devices:</h2>
  <ul>
    {#each availablePorts as [port, info]}
      <li>
        <p>USB Product Id: {info.usbProductId}</p>
        <p>Vendor Id: {info.usbVendorId}</p>
        <button on:click={send(port)}>Send</button>
      </li>
    {/each}
  </ul>

  <button on:click={connect}>Connect</button>
</main>

<style>
</style>
