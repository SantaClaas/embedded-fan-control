<script lang="ts">
  import BaudRateSelect, { type BaudRate } from "./BaudRateSelect.svelte";
  import PortDevices from "./PortDevices.svelte";

  export let port: SerialPort;
  export let info: Partial<SerialPortInfo>;

  let selectedRate: BaudRate = 9600;
  let isOpen = !!port.writable && !!port.readable;

  type SupportedParity = Exclude<ParityType, "mark" | "space">;
  let parity: SupportedParity = "none";
  const parityOptions: SupportedParity[] = ["none", "even", "odd"];

  async function openPort() {
    if (isOpen) return;

    await port.open({ baudRate: selectedRate, parity });

    isOpen = !!port.writable && !!port.readable;
  }

  async function closePort() {
    if (!isOpen) return;

    try {
      await port.close();
    } catch (error) {
      console.error("Could not close port:", error);
    }

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

  <BaudRateSelect
    bind:value={selectedRate}
    disabled={isOpen}
    id="baud-rate-port"
  />

  <label for="parity">Parity</label>
  <select id="parity" bind:value={parity}>
    {#each parityOptions as parity}
      <option value={parity}>{parity}</option>
    {/each}
  </select>

  <div>
    <button disabled={!isOpen} on:click={closePort}>Close</button>
    <button disabled={isOpen} on:click={openPort}>Open</button>
  </div>
</fieldset>

{#if isOpen}
  <PortDevices baudrate={selectedRate} {port} />
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

  /* Can't style option directly so we rely on inheritance */
  select#parity {
    text-transform: capitalize;
  }
</style>
