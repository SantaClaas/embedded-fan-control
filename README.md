# Caveats
**Packages can not be run from the workspace root.** You need run them from their respective directory.
This is due to the [fan-controller](fan-controller) package only compiling with the `thumbv6m-none-eabi` target which is
specified in [.config.toml](fan-controller/.cargo/config.toml) which will only be read by cargo when running from the
that package directory.  
There is currently an unstable [per-package-target](https://doc.rust-lang.org/cargo/reference/unstable.html#per-package-target) 
cargo feature in the works [on GitHub](https://github.com/rust-lang/cargo/issues/9406), but it does not support the
runner specified which is also required to run on a connected RP2040 pico.