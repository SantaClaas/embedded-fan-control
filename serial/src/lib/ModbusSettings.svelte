<script lang="ts">
  import BaudRateSelect, { type BaudRate } from "./BaudRateSelect.svelte";
  import Fan from "./Fan.svelte";
  import TemperatureSensor from "./TemperatureSensor.svelte";

  // Baudrate is set on serial port, not modbus
  export let baudrate: BaudRate;
  export let port: SerialPort;
  const temperatureSensorOption = "temperature-sensor";
  const fanOption = "fan";

  let selectedDevice: typeof temperatureSensorOption | typeof fanOption =
    "temperature-sensor";

  let address = 1;
</script>

<h3>Modbus</h3>
<fieldset class="device">
  <legend>Connected Device</legend>
  <BaudRateSelect value={baudrate} disabled />
  <label for="device-address">Address</label>
  <input
    id="device-address"
    type="number"
    inputmode="numeric"
    min="1"
    max="247"
    step="1"
    bind:value={address}
  />
  <label for="device-type">Type</label>
  <select id="device-type" bind:value={selectedDevice}>
    <option value={temperatureSensorOption}>Temperature Sensor</option>
    <option disabled value={fanOption}>Fan (Coming soon)</option>
  </select>
</fieldset>

{#if selectedDevice === "temperature-sensor"}
  <TemperatureSensor sensorAddress={address} {port} />
{:else if selectedDevice === "fan"}
  <Fan />
{/if}

<style>
  fieldset.device {
    display: grid;
    grid-template-columns: repeat(2, auto);
    gap: 1rem;
  }
</style>
