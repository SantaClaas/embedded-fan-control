> [!NOTE]
> This is an attempt at formatting what we found as documentation for the temparature sensor.
> The raw text it was formatted from is kept alongside it in
> [temperature-sensor-unformatted.txt](temperature-sensor-unformatted.txt). Unlike the RadiCal fan
> and the relay module, no manufacturer PDF exists for this device, so this file is the only
> documentation there is.

## Product parameters:

Working voltage: DC4-30V (the highest should not exceed 33V)

Maximum power: 0.2W

Working temperature: temperature -20℃+60℃, humidity 0%RH-100%RH

Control accuracy: temperature ±0.3℃(25℃), humidity ±3%RH(25℃)

Output interface: RS485 communication (standard MODBUS protocol and custom common protocol), see protocol description for details

Device address: 1-247 can be set, the default is 1
Baud rate: default 9600 (users can set by themselves), 8 data, 1 stop, no parity
Size: 60 _ 30 _ 18

## MODBUS protocol

### Function code used by the product:

|       |                                  |
| ----- | -------------------------------- |
| 0x03: | Read holding register            |
| 0x04: | Read input register              |
| 0x06: | Write a single holding register  |
| 0x10: | write multiple holding registers |

| Register type    | Register address | Data content                                  | Number of bytes |
| ---------------- | ---------------- | --------------------------------------------- | --------------- |
| Input register   | 0x0001           | Temperature value                             | 2               |
|                  | 0x0002           | Humidity value                                | 2               |
| Holding register | 0x0101           | Device address (1~247)                        | 2               |
|                  | 0x0102           | Baud rate 0:9600 1:14400 2:19200              | 2               |
|                  | 0x0103           | Temperature correction value (/10) -10.0~10.0 | 2               |
|                  | 0x0104           | Humidity correction value (/10) -10.0~10.0    | 2               |

### Modbus communication format:

#### Host sends data frame:

|               |               |                            |                           |                               |                              |               |              |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |

### The slave responds to the data frame:

|               |                        |                 |                           |                          |                           |                          |               |              |
| ------------- | ---------------------- | --------------- | ------------------------- | ------------------------ | ------------------------- | ------------------------ | ------------- | ------------ |
| Slave address | Response function code | Number of bytes | Register 1 data High byte | Register 1 data Low byte | Register N data High byte | Register N data Low byte | CRC High byte | CRC Low byte |

### MODBUS command frame

### The host reads the temperature command frame (0x04):

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x01                      | 0x00                          | 0x01                         | 0x60          | 0x0a         |

#### The slave responds to the data frame:

| Slave address | function code | Number of bytes | temperature High byte | temperature Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | --------------------- | -------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x02            | 0x01                  | 0x31                 | 0x79          | 0x74         |

Temperature value = 0x131, converted to decimal 305, actual temperature value = 305/10 = 30.5℃

> [!NOTE]
> The temperature is a signed hexadecimal number, temperature value=0xFF33, converted to decimal -205, actual temperature = -20.5℃;

#### The host reads the humidity command frame (0x04):

> [!WARNING]
> **Both frames in this humidity section had wrong check bytes in the source we transcribed from,
> and both have been corrected below.** The request was printed as `0xC1 0xCA` but `01 04 00 02 00
> 01` checksums to `0x90 0x0A`; the response was printed as `0xD1 0xBA` but `01 04 02 02 22`
> checksums to `0x38 0x49`.
>
> Every other frame in this file is correct, as is every frame in the relay manual. They were all
> checked against the CRC implementation in
> [serial/src/modbus/crc.ts](../serial/src/modbus/crc.ts) by `serial/src/modbus/crc.test.ts`, which
> is where the mistake surfaced. Do not "fix" these back without recomputing them.

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x02                      | 0x00                          | 0x01                         | 0x90          | 0x0A         |

#### The slave responds to the data frame:

| Slave address | function code | Number of bytes | humidity High byte | humidity Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | ------------------ | ----------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x02            | 0x02               | 0x22              | 0x38          | 0x49         |

Humidity value=0x222, converted to decimal 546, actual humidity value=546 / 10 = 54.6%;

### Continuously read the temperature and humidity command frame (0x04):

> [!NOTE]
> I believe the original author meant continuously as in "multiple" and not continuously as in "over time"

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x01                      | 0x00                          | 0x02                         | 0x20          | 0x0B         |

#### The slave responds to the data frame:

| Slave address | function code | Number of bytes | temperature High byte | temperature Low byte | humidity High byte | humidity Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | --------------------- | -------------------- | ------------------ | ----------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x04            | 0x01                  | 0x31                 | 0x02               | 0x22              | 0x2A          | 0xCE         |

### Read the content of the holding register (0x03):

#### Take reading the slave address as an example:

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x03          | 0x01                       | 0x01                      | 0x00                          | 0x01                         | 0xD4          | 0x0F         |

#### Slave response frame:

| Slave address | function code | Number of bytes | Slave address High byte | Slave address Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | ----------------------- | ---------------------- | ------------- | ------------ |
| 0x01          | 0x03          | 0x02            | 0x00                    | 0x01                   | 0x30          | 0x18         |

### Modify the content of the holding register (0x06):

#### Take the modification of the slave address as an example:

| Slave address | function code | Register address High byte | Register address Low byte | Register value High byte | Register value Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ------------------------ | ----------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x01                       | 0x01                      | 0x00                     | 0x08                    | 0xD4          | 0x0F         |

Modify slave address: 0x08 = 8

#### Slave response frame (same as sending):

| Slave address | function code | Register address High byte | Register address Low byte | Register value High byte | Register value Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ------------------------ | ----------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x01                       | 0x01                      | 0x00                     | 0x08                    | 0xD4          | 0x0F         |

### Continuously modify the holding register (0x10):

| Slave address | function code | initial address High byte | initial address Low byte | Number of registers High byte | Number of registers Low byte | Number of bytes | Register 1 high byte | Register 1 low byte | Register 2 high byte | Register 2 low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | ------------------------- | ------------------------ | ----------------------------- | ---------------------------- | --------------- | -------------------- | ------------------- | -------------------- | ------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x01                      | 0x01                     | 0x00                          | 0x02                         | 0x04            | 0x00                 | 0x20                | 0x25                 | 0x80                | 0x25          | 0x09         |

Modify slave address: 0x20 = 32
Baud rate: 0x2580 = 9600

#### Slave response frame:

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x00                       | 0x11                      | 0x00                          | 0x04                         | 0xD4          | 0x0F         |

## Normal version agreement

The default baud rate is 9600 (users can set by themselves),
8 bits of data,
1 bit of stop,
no parity RS485 communication

| Serial command | Description                                                                                      |
| -------------- | ------------------------------------------------------------------------------------------------ |
| READ           | Trigger a temperature and humidity report (27.4℃, 67.7% temperature 27.4℃ humidity 67.7%)        |
| AUTO           | Start the automatic temperature and humidity report function (Same as above)                     |
| STOP           | Stop the automatic reporting of temperature and humidity                                         |
| BR:XXXX        | Set the baud rate 9600~19200 (BR: 9600 baud rate is 9600)                                        |
| TC:XX.X        | Set temperature calibration (-10.0~10.0) (TC:02.0 temperature correction value is 2.0℃)          |
| HC:XX.X        | Set humidity calibration (-10.0~10.0) (HC:-05.1 Humidity correction value is -5.1%)              |
| HZ:XXX         | Set the temperature and humidity report rate (0.5,1,2,5,10) (HZ: 2 automatic reporting rate 2Hz) |
| PARAM          | Read current system settings，                                                                   |

PARAM instruction:
TC:0.0,HC:0.0,BR:9600,HZ:1 -> Temperature correction value 0.0 Humidity correction value 0.0 Baud rate 9600 Report rate 1Hz SLAVE_ADD:1 ->MODBUS slave address 0x01

---

# How the controller uses these sensors

Everything above is the transcription. What follows is this repository's own decisions about the
two of these that hang off the fan controller, which belong here rather than in the document they
came with.

## They share the relay's bus

The controller drives two Modbus buses and has no room for a third: the RP2040 has two UARTs, UART0
carries the fans and UART1 carries the relay module. The sensors are on UART1, alongside the relay.

That is forced rather than chosen, and it works out only because of the framing. The fans run
19_200 baud 8E1 and refuse to be anything else; these sensors answer 8N1, like the relay, and their
parity is not settable any more than the relay's is — so the fans' bus was never an option. Both
ship at 9600, which is what the bus is opened at, and
[fan-controller/src/temperature_sensor/mod.rs](../fan-controller/src/temperature_sensor/mod.rs)
fails the build if those two bit rates are ever changed apart.

Sharing the bus is why `relay_routine` no longer owns its Modbus client. It is behind the same
mutex and once lock the fans' client is behind, and every transaction — a coil write, a contact
read, a sensor poll — takes the lock for one exchange and releases it, so a device that has stopped
answering costs the others a timeout rather than a run of them.

## Addresses

| Device | Address |
|---|---|
| Relay module | `0xFF` |
| Temperature sensor 1 | `0x04` |
| Temperature sensor 2 | `0x05` |

Both sensors ship at `0x01`, and two devices at one address answer over each other, so **each
sensor has to be re-addressed before it is wired onto the bus** — one at a time, with nothing else
of the same address on the line. The [serial tool](../serial) does this: holding register `0x0101`,
which it presents as "Device address". The change is written to the device's flash and survives a
power cycle.

`0x04` and `0x05` continue the fans' `0x02`/`0x03` rather than starting over. The two buses could
not collide even if they shared a number, but one address for one device reads back more easily in
a log — the firmware's Modbus client prints `[Temperature 1]` and `[Temperature 2]` from the
address alone — and `0x01` is skipped for the same reason the fans skip it: it is the likely
factory default of whatever is added next.

## What is read, and how often

One transaction per sensor every 30 seconds, after a 10 second delay at boot that leaves the bus to
the relay's contact read. It is the "continuously read the temperature and humidity" frame above —
input registers `0x0001` and `0x0002` in one request — because a range costs the same round trip as
a single register, and reading both at once means the two values describe the same moment.

A failed poll is logged and dropped rather than retried. The next one is along in 30 seconds
carrying fresher values than a retry would, and a silent sensor never holds the bus while a relay
command waits behind it. That also absorbs the relay's power-up greeting, which spoils whatever
exchange it collides with whenever the *module* is powered — see [relay.md](relay.md).

## What Home Assistant is told

Two sensors per device, temperature and humidity, both reading one topic per device:

```
fan-controller/temperature-sensor-1/sensors/state
fan-controller/temperature-sensor-2/sensors/state
```

carrying `{"temperature":21.4,"humidity":54.6}`, which each sensor picks its value out of with a
value template. One publish per poll rather than two, the same shape the fans' readings are
published in.

The values are written with one decimal because that is the resolution the device has. The firmware
keeps them as signed tenths and never divides: an RP2040 has no floating point unit, and the
decimal point is put in only where the number is written out.

## What has not been checked

None of this has run against the hardware. The register addresses, the coding and the frames come
from a transcription that was wrong twice before, so the first thing worth doing with a sensor on
the bench is comparing a reading against something else in the same room — a value that decodes to
a plausible temperature but a nonsensical humidity would be the signature of a register table that
does not match this device.
