//! This build script copies the `memory.x` file from the crate root into
//! a directory where the linker can always find it at build time.
//! For many projects this is optional, as the linker always searches the
//! project root directory -- wherever `Cargo.toml` is. However, if you
//! are using a workspace or have a more complicated build setup, this
//! build script becomes required. Additionally, by requesting that
//! Cargo re-run the build script whenever `memory.x` is changed,
//! updating `memory.x` ensures a rebuild of the application with the
//! new memory settings.

use std::env::{self, VarError};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
enum BuildError {
    #[error("Failed to write memory.x file: {0}")]
    WriteMemoryXFile(#[from] std::io::Error),

    #[error("Failed to load .env file for Wifi credentials: {0}")]
    LoadEnvFile(#[from] dotenvy::Error),

    #[error("Missing variable {1} in .env file: {0}")]
    MissingEnvVar(VarError, &'static str),
}

fn ensure_memory_x_file() -> Result<(), BuildError> {
    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))?;
    println!("cargo:rustc-link-search={}", out.display());

    // By default, Cargo will re-run a build script whenever
    // any file in the project changes. By specifying `memory.x`
    // here, we ensure the build script is only re-run when
    // `memory.x` is changed.
    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");

    Ok(())
}

fn setup_wifi_credentials() -> Result<(), BuildError> {
    const WIFI_NETWORK: &str = "FAN_CONTROL_WIFI_NETWORK";
    const WIFI_PASSWORD: &str = "FAN_CONTROL_WIFI_PASSWORD";

    dotenvy::dotenv()?;
    let wifi_network =
        env::var(WIFI_NETWORK).map_err(|error| BuildError::MissingEnvVar(error, WIFI_NETWORK))?;
    let wifi_password =
        env::var(WIFI_PASSWORD).map_err(|error| BuildError::MissingEnvVar(error, WIFI_PASSWORD))?;

    println!("cargo:rustc-env=FAN_CONTROL_WIFI_NETWORK={wifi_network}");
    println!("cargo:rustc-env=FAN_CONTROL_WIFI_PASSWORD={wifi_password}");

    Ok(())
}

fn main() -> Result<(), BuildError> {
    ensure_memory_x_file()?;
    setup_wifi_credentials()?;
    Ok(())
}
