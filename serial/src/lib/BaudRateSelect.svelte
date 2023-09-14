<script lang="ts" context="module">
  export type BaudRate = 9600 | 14400 | 19200;
</script>

<script lang="ts">
  export let value: BaudRate;
  export let disabled: boolean | undefined = undefined;

  const selectableRates: BaudRate[] = [9600, 14400, 19200];
</script>

<label for="baud-rate-port">Baud rate</label>
<select id="baud-rate-port" bind:value {disabled}>
  {#each selectableRates as baudRate}
    <option value={baudRate}>{baudRate} Bit/sec</option>
  {/each}
</select>
