This is an attempt at formatting what we found as documentation for the temparature sensor

## Product parameters:

Working voltage: DC4-30V (the highest should not exceed 33V)

Maximum power: 0.2W

Working temperature: temperature -20℃+60℃, humidity 0%RH-100%RH

Control accuracy: temperature ±0.3℃(25℃), humidity ±3%RH(25℃)

Output interface: RS485 communication (standard MODBUS protocol and custom common protocol), see protocol description for details

Device address: 1-247 can be set, the default is 1
Baud rate: default 9600 (users can set by themselves), 8 data, 1 stop, no parity
Size: 60 _ 30 _ 18

## MODBUS protocol Function code used by the product:

0x03: Read holding register
0x04: Read input register
0x06: Write a single holding register
0x10: write multiple holding registers

| Register type                                 | Register address | Data content |
| --------------------------------------------- | ---------------- | ------------ |
| Number of bytes Input register                |                  | 0x0001       |
| Temperature value                             | 2                | 0x0002       |
| Humidity value                                | 2                |              |
| Holding register                              |                  | 0x0101       |
| Device address (1~247)                        | 2                | 0x0102       |
| Baud rate 0:9600 1:14400 2:19200              | 2                | 0x0103       |
| Temperature correction value (/10) -10.0~10.0 | 2                | 0x0104       |
| Humidity correction value (/10) -10.0~10.0    | 2                |              |

### Modbus communication format:

#### Host sends data frame:

Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte

### The slave responds to the data frame:

Slave address | Response function code | Number of bytes | Register 1 data High byte | Register 1 data Low byte | Register N data High byte | Register N data Low byte | CRC High byte | CRC Low byte，

### MODBUS command frame The host reads the temperature command frame (0x04):

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x01                      | 0x00                          | 0x01                         | 0x60          | 0x0a         |

### The slave responds to the data frame:

| Slave address | function code | Number of bytes | temperature High byte | temperature Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | --------------------- | -------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x02            | 0x01                  | 0x31                 | 0x79          | 0x74         |

Temperature value = 0x131, converted to decimal 305, actual temperature value = 305/10 = 30.5℃

> Note: The temperature is a signed hexadecimal number, temperature value=0xFF33, converted to decimal -205, actual temperature = -20.5℃;

### The host reads the humidity command frame (0x04):

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x02                      | 0x00                          | 0x01                         | 0xC1          | 0xCA         |

### The slave responds to the data frame:

| Slave address | function code | Number of bytes | humidity High byte | humidity Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | --------------- | ------------------ | ----------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x02            | 0x02               | 0x22              | 0xD1          | 0xBA         |

Humidity value=0x222, converted to decimal 546, actual humidity value=546 / 10 = 54.6%;

### Continuously read the temperature and humidity command frame (0x04):

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x04          | 0x00                       | 0x01                      | 0x00                          | 0x02                         | 0x20          | 0x0B         |

### The slave responds to the data frame:

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
Slave response frame (same as sending):
Slave address | function code | Register address High byte | Register address Low byte | Register value High byte | Register value Low byte | CRC High byte | CRC Low byte |
| ----------- | ------------- | -------------------------- | ------------------------- | ------------------------ | ----------------------- | ------------- | ------------ |
| 0x01 | 0x06 | 0x01 | 0x01 | 0x00 | 0x08 | 0xD4 | 0x0F |

Continuously modify the holding register (0x10):

| Slave address | function code | initial address High byte | initial address Low byte | Number of registers High byte | Number of registers Low byte | Number of bytes | Register 1 high byte | Register 1 low byte | Register 2 high byte | Register 2 low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | ------------------------- | ------------------------ | ----------------------------- | ---------------------------- | --------------- | -------------------- | ------------------- | -------------------- | ------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x01                      | 0x01                     | 0x00                          | 0x02                         | 0x04            | 0x00                 | 0x20                | 0x25                 | 0x80                | 0x25          | 0x09         |

Modify slave address: 0x20 = 32，Baud rate: 0x2580 = 9600
Slave response frame:

| Slave address | function code | Register address High byte | Register address Low byte | Number of registers High byte | Number of registers Low byte | CRC High byte | CRC Low byte |
| ------------- | ------------- | -------------------------- | ------------------------- | ----------------------------- | ---------------------------- | ------------- | ------------ |
| 0x01          | 0x06          | 0x00                       | 0x11                      | 0x00                          | 0x04                         | 0xD4          | 0x0F         |

Normal version agreement
The default baud rate is 9600 (users can set by themselves),
8 bits of data,
1 bit of stop,
no parity RS485 communication，

Serial command
| Description READ
| Trigger a temperature and humidity report (27.4℃, 67.7% temperature 27.4℃ humidity 67.7%) AUTO
| Start the automatic temperature and humidity report function (Same as above) STOP
| Stop the automatic reporting of temperature and humidity BR:XXXX
| Set the baud rate 9600~19200 (BR: 9600 baud rate is 9600) TC:XX.X
| Set temperature calibration (-10.0~10.0) (TC:02.0 temperature correction value is 2.0℃) HC:XX.X
| Set humidity calibration (-10.0~10.0) (HC:-05.1 Humidity correction value is -5.1%) HZ:XXX
| Set the temperature and humidity report rate (0.5,1,2,5,10) (HZ: 2 automatic reporting rate 2Hz) PARAM
| Read current system settings，

PARAM instruction: TC:0.0,HC:0.0,BR:9600,HZ:1 ->Temperature correction value 0.0 Humidity correction value 0.0 Baud rate 9600 Report rate 1Hz SLAVE_ADD:1 ->MODBUS slave address 0x01
