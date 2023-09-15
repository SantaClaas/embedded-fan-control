<script lang="ts">
  import BaudRateSelect, { type BaudRate } from "./BaudRateSelect.svelte";
  import ModbusSettings from "./ModbusSettings.svelte";

  export let port: SerialPort;
  export let info: Partial<SerialPortInfo>;

  let selectedRate: BaudRate = 9600;
  let isOpen = !!port.writable && !!port.readable;

  async function openPort() {
    if (isOpen) return;

    await port.open({ baudRate: selectedRate });

    isOpen = !!port.writable && !!port.readable;
  }

  async function closePort() {
    if (!isOpen) return;

    await port.close();

    isOpen = !!port.writable && !!port.readable;
  }
</script>

<fieldset class="port-settings">
  <legend>Serial Port Settings</legend>
  <label for="vendor-id">USB Vendor Id</label>
  <input
    id="vendor-id"
    type="number"
    value={info.usbVendorId}
    readonly
    disabled
  />
  <label for="product-id">USB Product Id</label>
  <input
    id="product-id"
    type="number"
    value={info.usbProductId}
    readonly
    disabled
  />

  <BaudRateSelect bind:value={selectedRate} disabled={isOpen} />

  <div>
    <button disabled={!isOpen} on:click={closePort}>Close</button>
    <button disabled={isOpen} on:click={openPort}>Open</button>
  </div>
</fieldset>

{#if isOpen}
  <ModbusSettings baudrate={selectedRate} {port} />
{/if}

<style>
  fieldset.port-settings {
    display: grid;
    gap: 1rem;
    grid-template-columns: repeat(2, auto);

    & div {
      justify-self: end;
      grid-column: 2 / 3;
    }
  }
</style>
