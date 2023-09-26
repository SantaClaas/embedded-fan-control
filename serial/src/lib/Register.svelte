<script lang="ts" generics="T">
  import { InputRegister, HoldingRegister } from "./modbus";
  export let label: string;
  export let register: InputRegister<T> | HoldingRegister<T>;
  export let disabled: boolean;

  let value: T | "error reading";
  const id = self.crypto.randomUUID();

  async function read() {
    if (disabled) return;
    disabled = true;
    try {
      const result = await register.read();
      value = result ?? "error reading";
    } finally {
      disabled = false;
    }
  }

  async function write() {
    if (disabled || value === "error reading" || !("write" in register)) return;

    disabled = true;

    try {
      await register.write(value);
    } finally {
      disabled = false;
    }
  }
</script>

<label for={id}>{register.label ?? label}</label>

{#if register instanceof HoldingRegister}
  {#if register.input}
    {#if "options" in register.input}
      <select {id} {disabled}>
        {#if !value}
          <option value={null} />
        {/if}
        {#each Object.entries(register.input.options) as [value, label]}
          <option {value}>{label}</option>
        {/each}
      </select>
    {:else if "min" in register.input}
      <input
        bind:value
        type="number"
        {id}
        {disabled}
        min={register.input.min}
        max={register.input.max}
        step={register.input.step}
      />
    {/if}
  {:else}
    <input bind:value {id} {disabled} />
  {/if}
{:else}
  <input bind:value {id} readonly />
{/if}

<button on:click={read} {disabled}>Read</button>
{#if register instanceof HoldingRegister}
  <button on:click={write} {disabled}>Write</button>
{/if}
