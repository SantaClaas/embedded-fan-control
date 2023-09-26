<script lang="ts">
  import BaudRateSelect, { type BaudRate } from "./BaudRateSelect.svelte";

  import DeviceView from "./Device.svelte";

  import { Fan } from "./fan";
  import { TemperatureSensor } from "./temperatureSensor";

  // Baudrate is set on serial port, not modbus
  export let baudrate: BaudRate;
  export let port: SerialPort;
  const temperatureSensorOption = "temperature-sensor";
  const fanOption = "fan";

  let selectedDevice: typeof temperatureSensorOption | typeof fanOption =
    "temperature-sensor";

  $: device =
    selectedDevice === "fan"
      ? new Fan(port, address)
      : new TemperatureSensor(port, address);

  let address = 1;
  let disabled = false;
</script>

<h3>Modbus</h3>
<fieldset class="device">
  <legend>Connected Device</legend>
  <BaudRateSelect value={baudrate} disabled id="baud-rate-device" />
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
    <option value={fanOption}>Fan</option>
  </select>
</fieldset>

<DeviceView {device} {disabled} />

<style>
  fieldset.device {
    display: grid;
    grid-template-columns: repeat(2, auto);
    gap: 1rem;
  }
</style>
