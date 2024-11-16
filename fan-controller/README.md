# How to run

install [probe.rs](https://probe.rs) `cargo run`

# TODO

- [ ] Read fan speed on boot in case fans were already running
- [x] Debounce button tap
- [ ] Retry if there was an error writing to one or two fans
- [ ] Confirm fan speed is set on homeassistant and retry otherwise (there might be network interference)
- [ ] Flash LEDs for one second after boot to indicate if they work
- [ ] Make Button press pick up state that was changed through Homeassistant and not keep its own fan state
