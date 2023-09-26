<script lang="ts" context="module">
  export type BaudRate = 9600 | 14400 | 19200;
</script>

<script lang="ts">
  export let value: BaudRate;
  export let disabled: boolean | undefined = undefined;
  export let id: string;
  const selectableRates: BaudRate[] = [9600, 14400, 19200];
</script>

<label for={id}>Baud rate</label>
<select {id} bind:value {disabled}>
  {#each selectableRates as baudRate}
    <option value={baudRate}>{baudRate} Bit/sec</option>
  {/each}
</select>
