<script lang="ts">
  import BaudRateSelect, { type BaudRate } from "./BaudRateSelect.svelte";

  import DeviceView from "./DeviceView.svelte";

  import { TemperatureSensor } from "./devices/temperatureSensor";
  import type { Device } from "./modbus";
  import { supportedDevices, type DeviceConstructor } from "./supportedDevices";

  // Baudrate is set on serial port, not modbus
  export let baudrate: BaudRate;
  export let port: SerialPort;

  let newDeviceAddress = 1;
  let disabled = false;

  let devices: [name: string, Device][] = [];

  let deviceConstructor: DeviceConstructor = TemperatureSensor;
  const usedAddresses: Set<number> = new Set();

  function getName(deviceConstructor: DeviceConstructor): string | undefined {
    // Could optimize linear search but that seems overkill at the moment
    for (const [key, constructor] of Object.entries(supportedDevices)) {
      if (constructor === deviceConstructor) return key;
    }

    return undefined;
  }

  function addDevice() {
    if (usedAddresses.has(newDeviceAddress)) return;

    const newDevice = new deviceConstructor(port, newDeviceAddress);
    const name = getName(deviceConstructor) ?? "Device";
    devices.push([name, newDevice]);
    usedAddresses.add(newDeviceAddress);
    devices = devices;

    // Find next available address
    for (let address = 1; address < 248; address++) {
      if (usedAddresses.has(address)) continue;

      newDeviceAddress = address;
      break;
    }
  }

  function validateInput({
    currentTarget,
  }: {
    currentTarget: HTMLInputElement;
  }) {
    if (!(currentTarget instanceof HTMLInputElement)) return;

    const message = usedAddresses.has(newDeviceAddress)
      ? "This address is already used"
      : "";

    currentTarget.setCustomValidity(message);
  }
</script>

<h3>Modbus</h3>
<form class="device" on:submit|preventDefault={addDevice}>
  <fieldset class="device">
    <legend>Add Device</legend>
    <BaudRateSelect value={baudrate} disabled id="baud-rate-device" />
    <label for="device-address">Address</label>
    <input
      id="device-address"
      type="number"
      inputmode="numeric"
      min="1"
      max="247"
      step="1"
      bind:value={newDeviceAddress}
      on:input={validateInput}
    />

    <label for="device-type">Type</label>
    <select bind:value={deviceConstructor} id="device-type">
      {#each Object.entries(supportedDevices) as [name, constructor]}
        <option value={constructor}>{name}</option>
      {/each}
    </select>

    <button type="submit">Add</button>
  </fieldset>
</form>

{#each devices as [name, device]}
  <hgroup>
    <h4>{name}</h4>
    <p>Address: {device.address}</p>
  </hgroup>
  <DeviceView {device} {disabled} />
{/each}

<style>
  fieldset.device {
    display: grid;
    grid-template-columns: repeat(2, auto);
    gap: 1rem;

    & > button {
      grid-column: 2 / 3;
    }
  }
</style>
