import { Fan } from "./devices/fan";
import { TemperatureSensor } from "./devices/temperatureSensor";
import type { Device } from "./modbus";

// export interface DeviceConstructor {
//   new (port: SerialPort, address: number): Device;
// }

export const supportedDevices = {
  "Temperature Sensor": TemperatureSensor,
  Fan: Fan,
};

type ValueOf<T> = T[keyof T];
export type DeviceConstructor = ValueOf<typeof supportedDevices>;
