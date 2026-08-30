# Caveats
**Packages can not be run from the workspace root.** You need run them from their respective directory.
This is due to the [fan-controller](fan-controller) package only compiling with the `thumbv6m-none-eabi` target which is
specified in [.config.toml](fan-controller/.cargo/config.toml) which will only be read by cargo when running from the
that package directory.
There is currently an unstable [per-package-target](https://doc.rust-lang.org/cargo/reference/unstable.html#per-package-target)
cargo feature in the works [on GitHub](https://github.com/rust-lang/cargo/issues/9406), but it does not support the
runner specified which is also required to run on a connected RP2040 pico.

# Updating the Raspberry Pi Pico W as probe
If probe-rs gives a warning that the probe firmware is too old use these links
https://www.raspberrypi.com/documentation/microcontrollers/debug-probe.html#updating-the-firmware-on-the-debug-probe
https://github.com/raspberrypi/debugprobe/releases/tag/debugprobe-v2.2.3

# Debugging MQTT discovery payload in Home Assistant
If the MQTT discovery payload contains invalid values, it will usually be logged at under [Settings > System Log](http://homeassistant:8123/config/logs)

# Wi-Fi firmware
[fan-controller/cyw43-firmware](fan-controller/cyw43-firmware) holds the CYW43439 blobs the Pico W's
Wi-Fi chip needs, taken from Infineon's
[wifi-host-driver](https://github.com/Infineon/wifi-host-driver/tree/master/WiFi_Host_Driver/resources/firmware/COMPONENT_43439).
They are committed rather than fetched or submoduled, because `include_bytes!` in
[main.rs](fan-controller/src/main.rs) bakes them into the binary at compile time — a checkout
without them does not build at all.

Redistributing them is permitted. They are covered by the
[Infineon Permissive Binary License](fan-controller/cyw43-firmware/LICENSE-permissive-binary-license-1.0.txt),
which allows redistribution in binary form as long as the copyright notice and disclaimer are
provided with them. That is what the license file and the directory's own README are for, so keep
both next to the blobs.

# Manufacturer documentation submodule
[docs/manufacturer](docs/manufacturer) is a submodule pointing at the private
[fan-documentation](https://github.com/SantaClaas/fan-documentation) repo, which holds the
ebm-papst fan manuals and other possibly copyrighted manufacturer material kept out of this
public repo. Run `git submodule update --init` after cloning to fetch it (requires access to
that private repo).
