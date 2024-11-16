# Documentation

Not all of the information but I hope to write down most of it.

## Status LEDs

The status LEDs indicate the fan speed.
| LED 1 | LED 2 | Status |
|-------|-------|---------------------------|
| Off | Off | Fan and/or controller off |
| On | Off | Fan speed low |
| Off | On | Fan speed medium |
| On | On | Fan speed max |

## Homeassistant integration

### 1. Join WiFi

The controller automatically joins the WiFi network that is configured.
The network name and passowrd is currently configured through environment variables at compile time and gets flashed onto the device. Plan is to make it configurable through a web interface. But there is no guarantee this will work out.

### 2. Homeassistant discovery

After successfully joining the network it tries to look up Homeassistant under the `homeassistant` name and tries to connect to it.
Homeassistant needs to have the MQTT broker installed as the controller uses MQTT to connect to homeassitant and send data between them.
After successful connection to the MQTT broker, the controller sends a discovery packet as defined by Homeassistant and the device should appear in Homeassistant on the dashboard when using the default Homeassistant configuration.
