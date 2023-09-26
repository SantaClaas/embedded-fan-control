<script lang="ts">
  import * as sensor from "./temperatureSensor";
  import { TemperatureSensor } from "./temperatureSensor";

  export let port: SerialPort;
  export let sensorAddress: number;

  const sensor2 = new TemperatureSensor(port, sensorAddress);

  type Result<T> = "error reading" | "error writing" | T;
  let temperature: null | Result<number> = null;

  let humidity: null | Result<number> = null;
  let address: null | Result<number> = null;
  let baudRate: null | Result<number> = null;
  let temperatureCorrection: null | Result<number> = null;
  let humidityCorrection: null | Result<number> = null;

  // Don't allow multiple operations at the same time
  let isBlocked = false;
  function timeout() {
    return AbortSignal.timeout(5_000);
  }

  async function readTemperature() {
    if (isBlocked) return;

    isBlocked = true;

    const result = await sensor2.inputRegisters.temperature.read(timeout());
    temperature = result ?? "error reading";
    isBlocked = false;
  }

  async function readHumidity() {
    if (isBlocked) return;

    isBlocked = true;
    const result = await sensor2.inputRegisters.humidity.read(timeout());
    humidity = result ?? "error reading";
    isBlocked = false;
  }

  async function readAddress() {
    if (isBlocked) return;

    isBlocked = true;

    const result = await sensor2.holdingRegisters.deviceAddress.read(timeout());
    address = result ?? "error reading";
    isBlocked = false;
  }

  async function writeAddress() {
    if (!sensor.isValidAddress(address) || isBlocked) return;

    isBlocked = true;
    const isSuccess = await sensor2.holdingRegisters.deviceAddress.write(
      address,
      timeout()
    );
    address = isSuccess ? address : "error writing";
    isBlocked = false;
  }

  async function readBaudRate() {
    if (isBlocked) return;

    isBlocked = true;

    const result = await sensor2.holdingRegisters.baudRate.read(timeout());
    baudRate = result ?? "error reading";

    isBlocked = false;
  }

  async function writeBaudRate() {
    if (isBlocked || !sensor.isValidBaudRate(baudRate)) return;

    isBlocked = true;
    const isSuccess = await sensor2.holdingRegisters.baudRate.write(
      baudRate,
      timeout()
    );
    baudRate = isSuccess ? baudRate : "error writing";
    isBlocked = false;
  }

  async function readTemperatureCorrection() {
    if (isBlocked) return;

    isBlocked = true;
    const result = await sensor2.holdingRegisters.temperatureCorrection.read(
      timeout()
    );
    temperatureCorrection = result ?? "error reading";
    isBlocked = false;
  }

  async function writeTemperatureCorrection() {
    if (!sensor.isValidCorrectionValue(temperatureCorrection) || isBlocked)
      return;

    isBlocked = true;
    const isSuccess =
      await sensor2.holdingRegisters.temperatureCorrection.write(
        temperatureCorrection,
        timeout()
      );
    temperatureCorrection = isSuccess ? temperatureCorrection : "error writing";
    isBlocked = false;
  }

  async function readHumidityCorrection() {
    if (isBlocked) return;

    isBlocked = true;
    const result = await sensor2.holdingRegisters.humidityCorrection.read(
      timeout()
    );
    humidityCorrection = result ?? "error reading";
    isBlocked = false;
  }

  async function writeHumidityCorrection() {
    if (!sensor.isValidCorrectionValue(humidityCorrection) || isBlocked) return;

    isBlocked = true;
    const isSuccess = await sensor2.holdingRegisters.humidityCorrection.write(
      humidityCorrection,
      timeout()
    );
    humidityCorrection = isSuccess ? humidityCorrection : "error writing";
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
  <span>
    Note: The active device address and baud rate is read from holding register
    register after device is powered on. Therefore changing these values does
    not apply until after next power cycle.
  </span>
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
  <button on:click={writeAddress}>Write</button>

  <label for="baud-rate-register">Baud rate</label>
  <select id="baud-rate-register" bind:value={baudRate}>
    {#if !baudRate}
      <option value={null} />
    {/if}
    {#each Object.entries(sensor.supportedBaudRates) as [value]}
      <option {value}>{value} Bits/sec</option>
    {/each}
  </select>
  <button on:click={readBaudRate}>Read</button>
  <button on:click={writeBaudRate}>Write</button>

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
  <button on:click={writeHumidityCorrection}>Write</button>
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
