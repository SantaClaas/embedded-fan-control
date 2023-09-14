<script lang="ts">
  import BaudRateSelect from "./BaudRateSelect.svelte";
  import type { BaudRate } from "./BaudRateSelect.svelte";
  import * as sensor from "./temperatureSensor";

  export let port: SerialPort;
  export let sensorAddress: number;

  type ReadResult<T> = "error reading" | T;
  let temperature: null | ReadResult<number> = null;

  let humidity: null | ReadResult<number> = null;
  let address: null | ReadResult<number> = null;
  let baudRate: null | ReadResult<number> = null;
  let temperatureCorrection: null | ReadResult<number> = null;
  let humidityCorrection: null | ReadResult<number> = null;

  // Don't allow multiple operations at the same time
  let isBlocked = false;

  async function readTemperature() {
    if (isBlocked) return;

    isBlocked = true;
    temperature =
      (await sensor.readTemperature(port, sensorAddress)) ?? "error reading";
    isBlocked = false;
  }

  async function readHumidity() {
    if (isBlocked) return;

    isBlocked = true;
    humidity =
      (await sensor.readHumidity(port, sensorAddress)) ?? "error reading";

    isBlocked = false;
  }

  async function readAddress() {
    if (isBlocked) return;

    isBlocked = true;
    address =
      (await sensor.readAddress(port, sensorAddress)) ?? "error reading";

    isBlocked = false;
  }
  async function readBaudRate() {
    if (isBlocked) return;

    isBlocked = true;
    baudRate =
      (await sensor.readBaudRate(port, sensorAddress)) ?? "error reading";
    isBlocked = false;
  }

  async function readTemperatureCorrection() {
    if (isBlocked) return;

    isBlocked = true;
    temperatureCorrection =
      (await sensor.readTemperatureCorrection(port, sensorAddress)) ??
      "error reading";

    isBlocked = false;
  }

  async function writeTemperatureCorrection() {
    console.log("hug", temperatureCorrection, isBlocked);
    if (
      temperatureCorrection === null ||
      temperatureCorrection === "error reading" ||
      isBlocked
    )
      return;

    const result = await sensor.writeTemperatureCorrection(
      port,
      sensorAddress,
      temperatureCorrection
    );
  }

  async function readHumidityCorrection() {
    if (isBlocked) return;

    isBlocked = true;
    humidityCorrection =
      (await sensor.readHumidityCorrection(port, sensorAddress)) ??
      "error reading";

    isBlocked = false;
  }
</script>

<h4>Temperature Sensor</h4>
<fieldset class="input-registers" disabled={isBlocked}>
  <legend>Input Registers</legend>
  <label for="temperature">Temperature (℃)</label>
  <input
    id="temperature"
    type="text"
    readonly
    disabled
    bind:value={temperature}
  />
  <button on:click={readTemperature}>Read</button>
  <label for="humidity">Humidity (%)</label>
  <input id="humidity" type="text" readonly disabled bind:value={humidity} />
  <button on:click={readHumidity}>Read</button>
</fieldset>

<fieldset class="holding-registers" disabled={isBlocked}>
  <legend>Holding Registers</legend>
  <label for="device-address-register">Address</label>
  <input
    id="device-address-register"
    type="number"
    inputmode="numeric"
    min="1"
    max="247"
    step="1"
    bind:value={address}
  />
  <button on:click={readAddress}>Read</button>
  <button>Write</button>
  <label for="baud-rate-register">Baud rate</label>
  <select id="baud-rate-register" bind:value={baudRate}>
    {#if !baudRate}
      <option value={null} />
    {/if}
    {#each sensor.supportedBaudRates as value}
      <option {value}>{value} Bits/sec</option>
    {/each}
  </select>
  <button on:click={readBaudRate}>Read</button>
  <button>Write</button>
  <label for="temperature-correction">Temperature Correction Value</label>
  <input
    id="temperature-correction"
    type="number"
    inputmode="decimal"
    max="10"
    min="-10"
    step="0.01"
    bind:value={temperatureCorrection}
  />
  <button on:click={readTemperatureCorrection}>Read</button>
  <button on:click={writeTemperatureCorrection}>Write</button>

  <label for="humidity-correction">Humidity Correction Value</label>
  <input
    id="humidity-correction"
    type="number"
    inputmode="decimal"
    max="10"
    min="-10"
    step="0.01"
    bind:value={humidityCorrection}
  />
  <button on:click={readHumidityCorrection}>Read</button>
  <button>Write</button>
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
  }
</style>
