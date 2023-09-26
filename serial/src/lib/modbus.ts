import { crc16 } from "./crc";

export enum FunctionCode {
  ReadHoldingRegister = 0x03,
  ReadInputRegister = 0x04,
  WriteSingleHoldingRegister = 0x06,
  Diagnostics = 0x08,
  WriteMultipleHoldingRegisters = 0x10,
}

/**
 * Validates that the given value is a valid device address between 1 and 247
 */
function isValidAddress(value: any): value is number {
  // Device address (1-247), 0 is broadcast
  return Number.isInteger(value) && value < 248 && value > 0;
}

/**
 * Validates a response to determine that the response is intended for us as it is a shared bus medium
 */
function isReadResponseValid(
  response: { view: DataView; frame: Uint8Array },
  expectedAddress: number,
  expectedFunctionCode: FunctionCode,
  expectedDataLength: number
) {
  const crcStartIndex = response.frame.byteLength - 2;
  const crc = crc16(response.frame.subarray(0, crcStartIndex));
  const expectedCrc = response.view.getUint16(5);

  if (crc !== expectedCrc) return false;

  const slaveAddress = response.view.getUint8(0);
  if (expectedAddress !== slaveAddress) return false;

  const functionCode = response.view.getUint8(1);
  if (functionCode !== expectedFunctionCode) return false;

  const dataLength = response.view.getUint8(2);

  return dataLength === expectedDataLength && 2 + dataLength < crcStartIndex;
}

/**
 * Validates that the response to a write is valid, that is the response mirrors the command
 */
function isWriteResponseValid(frame: Uint8Array, response: Uint8Array) {
  if (frame.length !== response.length) return false;

  for (let index = 0; index < frame.length; index++) {
    if (frame[index] !== response[index]) return false;
  }

  return true;
}

type SerializeFunction<T> = (value: T) => number;
type DeserializeFunction<T> = (view: DataView) => T;

type RegisterDefinition<T> = {
  /**
   * An optional human readable label or name for the register
   */
  label?: string;

  /**
   * A valid register address is a unsigned 16 bit number
   */
  address: number;

  /**
   * The length in bytes of the data contained in the register
   */
  length: number;

  /**
   * A deserializing function that allows customization how the value is read and allows interpretation of the binary
   */
  deserialize: DeserializeFunction<T>;
};

type InputRegisterDefinition<T> = RegisterDefinition<T>;

/**
 * Input options define what values can be put in a holding register before serialization.
 * This is first and foremost intended to validate values that should be written to the holding register but can be used
 * to defining HTML input elements
 */
export type InputOptions =
  | {
      options: { [key: number]: string };
    }
  | { min: number; max: number; step: number };
type HoldingRegisterDefinition<T> = RegisterDefinition<T> & {
  serialize: SerializeFunction<T>;
  isValid?(value: T): value is T;
  input?: InputOptions;
};

export class InputRegister<T> {
  #device: Device;
  #definition: InputRegisterDefinition<T>;

  get label(): string | undefined {
    return this.#definition.label;
  }

  constructor(device: Device, definition: InputRegisterDefinition<T>) {
    this.#device = device;
    this.#definition = definition;
  }

  async read(signal?: AbortSignal): Promise<T | undefined> {
    if (signal?.aborted) return;

    const length = 8;
    const crcOffset = length - 2;
    const frame = new ArrayBuffer(length);

    const view = new DataView(frame);
    // Address
    view.setUint8(0, this.#device.address);
    // Function
    view.setUint8(1, FunctionCode.ReadInputRegister);
    // Register address
    view.setUint16(2, this.#definition.address);
    // Number of registers
    view.setUint16(4, 1);
    // CRC
    const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
    view.setUint16(crcOffset, crc);

    try {
      await this.#device.write(frame, signal);
    } catch (error: unknown) {
      console.warn("Non-fatal write error:", error);
      return;
    }

    let value;
    try {
      value = await this.#device.read(signal);
    } catch (error: unknown) {
      console.warn("Non-fatal read error:", error);
      return;
    }

    if (!value) return;

    const response = { view: new DataView(value.buffer), frame: value };

    if (
      !isReadResponseValid(
        response,
        this.#device.address,
        FunctionCode.ReadInputRegister,
        this.#definition.length
      )
    )
      return;

    const deserializationView = new DataView(
      value.buffer,
      3,
      this.#definition.length
    );
    return this.#definition.deserialize(deserializationView);
  }
}

export class HoldingRegister<T> {
  #device: Device;
  #definition: HoldingRegisterDefinition<T>;

  get input(): InputOptions | undefined {
    return this.#definition.input;
  }

  get label(): string | undefined {
    return this.#definition.label;
  }

  constructor(device: Device, definition: HoldingRegisterDefinition<T>) {
    this.#device = device;
    this.#definition = definition;
  }

  async read(signal?: AbortSignal): Promise<T | undefined> {
    if (signal?.aborted) return;
    const length = 8;
    const crcOffset = length - 2;
    const frame = new ArrayBuffer(length);

    const view = new DataView(frame);
    // Address
    view.setUint8(0, this.#device.address);
    // Function
    view.setUint8(1, FunctionCode.ReadHoldingRegister);
    // Register address
    view.setUint16(2, this.#definition.address);
    // Number of registers
    view.setUint16(4, 1);
    // CRC
    const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
    view.setUint16(crcOffset, crc);

    try {
      await this.#device.write(frame, signal);
    } catch (error: unknown) {
      console.warn("Non-fatal write error:", error);
      return;
    }

    let value;
    try {
      value = await this.#device.read(signal);
    } catch (error: unknown) {
      console.warn("Non-fatal read error:", error);
      return;
    }

    if (!value) return;

    const response = { view: new DataView(value.buffer), frame: value };
    if (
      !isReadResponseValid(
        response,
        this.#device.address,
        FunctionCode.ReadHoldingRegister,
        this.#definition.length
      )
    )
      return;

    const deserializationView = new DataView(
      value.buffer,
      3,
      this.#definition.length
    );

    return this.#definition.deserialize(deserializationView);
  }

  async write(value: T, signal?: AbortSignal) {
    if (
      signal?.aborted ||
      (this.#definition.isValid && !this.#definition.isValid(value))
    )
      return false;
    const length = 8;
    const crcOffset = length - 2;
    const frame = new Uint8Array(length);
    const view = new DataView(frame.buffer);
    // Address
    view.setUint8(0, this.#device.address);
    // Function
    view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
    // Register address
    view.setUint16(2, this.#definition.address);
    // Register value
    view.setInt16(4, this.#definition.serialize(value));
    // CRC
    const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
    view.setUint16(crcOffset, crc);

    try {
      await this.#device.write(frame, signal);
    } catch (error: unknown) {
      console.warn("Non-fatal write error:", error);
      return false;
    }

    let responseFrame;
    try {
      responseFrame = await this.#device.read(signal);
    } catch (error: unknown) {
      console.warn("Non-fatal read error:", error);
      return false;
    }

    if (!responseFrame) return false;

    return isWriteResponseValid(frame, responseFrame);
  }
}

export abstract class Device {
  address: number;
  #port: SerialPort;

  static name: string;
  abstract inputRegisters: { readonly [name: string]: InputRegister<any> };
  abstract holdingRegisters: {
    readonly [name: string]: HoldingRegister<any>;
  };

  /**
   *
   * @param port The serial port used to write to write the holding register on the modbus device
   * @param address the 8 bit modbus device address
   */
  constructor(port: SerialPort, address: number) {
    this.address = address;
    this.#port = port;
  }

  /**
   *
   * @param definition the object describing the register
   */
  protected createInputRegister<T>(
    definition: InputRegisterDefinition<T>
  ): InputRegister<T> {
    return new InputRegister<T>(this, definition);
  }

  /**
   *
   * @param definition the object describing the register
   */
  protected createHoldingRegister<T>(definition: HoldingRegisterDefinition<T>) {
    return new HoldingRegister<T>(this, definition);
  }

  async write(frame: Uint8Array | ArrayBuffer, signal?: AbortSignal) {
    const writer = this.#port.writable.getWriter();
    function cancel() {
      writer.abort(signal?.reason);
    }

    signal?.addEventListener("abort", cancel, { once: true });

    try {
      await writer.write(frame);
    } finally {
      signal?.removeEventListener("abort", cancel);
      writer.releaseLock();
    }
  }

  async read(signal?: AbortSignal): Promise<Uint8Array | undefined> {
    if (signal?.aborted) return;

    const reader = this.#port.readable.getReader();
    function cancel() {
      reader.cancel(signal?.reason);
    }

    signal?.addEventListener("abort", cancel, { once: true });

    try {
      const { value, done } = await reader.read();

      if (done) {
        console.log("Done reading");
        return;
      }

      if (!value) {
        console.log("No value");
        return;
      }

      if (!(value instanceof Uint8Array))
        throw new Error("Expected value read to be binary");

      console.log("Read value: ", value);
      return value;
    } finally {
      signal?.removeEventListener("abort", cancel);
      reader.releaseLock();
    }
  }
}
