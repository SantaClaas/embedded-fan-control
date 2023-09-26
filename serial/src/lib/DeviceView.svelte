<script lang="ts">
  import Register from "./Register.svelte";
  import type { Device } from "./modbus";

  export let device: Device;

  export let disabled: boolean;
</script>

<fieldset class="input-registers" {disabled}>
  <legend>Input registers</legend>
  {#each Object.entries(device.inputRegisters) as [label, register]}
    <Register bind:disabled {register} {label} />
  {/each}
</fieldset>
<fieldset class="holding-registers" {disabled}>
  <legend>Holding registers</legend>
  {#each Object.entries(device.holdingRegisters) as [label, register]}
    <Register bind:disabled {register} {label} />
  {/each}
</fieldset>

<style>
  fieldset {
    display: grid;
    gap: 1rem;
  }

  fieldset.input-registers {
    grid-template-columns: repeat(3, auto);
  }

  fieldset.holding-registers {
    grid-template-columns: repeat(4, auto);

    & span {
      grid-column: 1 / 5;
      /* margin-block: 0; */
    }
  }
</style>
