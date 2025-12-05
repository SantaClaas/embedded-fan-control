# How to run

install [probe.rs](https://probe.rs) `cargo run`

# TODO

- [ ] Read fan speed on boot in case fans were already running
- [x] Debounce button tap
- [ ] Retry if there was an error writing to one or two fans
- [ ] When retrying fails after a certain while, try reset the other fan to not create an underpressure or overpressure in the house.
- [ ] Confirm fan speed is set on homeassistant and retry otherwise (there might be network interference). Can implement MQTT QoS for that
- [x] Flash LEDs for one second after boot to indicate if they work
- [ ] Make Button press pick up state that was changed through Homeassistant and not keep its own fan state
- [ ] Read temperature sensors
- [ ] Switch to only using refactored send for modbus
- [ ] [Reboot](https://github.com/embassy-rs/embassy/blob/f8685560531fcecb2f4327a490ec4df4f2b190f6/examples/rp/src/bin/rtc.rs#L50) on error after retries or connection lost. Or try reconnect in background.
- [ ] Try out bundling all channels in a sort of event bus like actor model

## Aspirational TODOs

- [ ] Make fan configurable through a web interface if it is not configured and set up with WiFi yet
