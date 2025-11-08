//! This build script copies the `memory.x` file from the crate root into
//! a directory where the linker can always find it at build time.
//! For many projects this is optional, as the linker always searches the
//! project root directory -- wherever `Cargo.toml` is. However, if you
//! are using a workspace or have a more complicated build setup, this
//! build script becomes required. Additionally, by requesting that
//! Cargo re-run the build script whenever `memory.x` is changed,
//! updating `memory.x` ensures a rebuild of the application with the
//! new memory settings.
#![allow(unused)]

use std::collections::HashMap;
use std::env::{self, VarError};
use std::fmt::format;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::string::FromUtf8Error;

use home_assistant_discovery::{Component, Device, DiscoveryPayload, ListOrString, Origin};
use mqtt::QualityOfService;
use ruff_python_ast::{DictItem, Expr, Stmt};

#[derive(Debug, thiserror::Error)]
enum GitHashError {
    #[error("Failed to execute git command: {0}")]
    CommandExecution(std::io::Error),
    #[error("Failed to parse git hash command output: {0}")]
    ParseHash(FromUtf8Error),
}

#[derive(Debug, thiserror::Error)]
enum BuildError {
    #[error("Failed to write memory.x file: {0}")]
    WriteMemoryXFile(#[from] std::io::Error),

    #[error("Failed to load .env file for Wifi credentials: {0}")]
    LoadEnvFile(#[from] dotenvy::Error),

    #[error("Missing variable {1} in .env file: {0}")]
    MissingEnvVar(VarError, &'static str),

    #[error("Error with discovery payload")]
    DiscoveryPayloadError(#[from] DiscoveryPayloadError),

    #[error("Failed to get git hash: {0}")]
    GitHash(#[from] GitHashError),
}

#[derive(Debug, thiserror::Error)]
enum DiscoveryPayloadError {
    #[error("Failed to read discovery payload file at \"{1}\": {0}")]
    ReadDiscoveryPayload(std::io::Error, &'static str),
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

fn setup_configuration() -> Result<(), BuildError> {
    const WIFI_NETWORK: &str = "FAN_CONTROL_WIFI_NETWORK";
    const WIFI_PASSWORD: &str = "FAN_CONTROL_WIFI_PASSWORD";
    const MQTT_BROKER_USERNAME: &str = "FAN_CONTROL_MQTT_BROKER_USERNAME";
    const MQTT_BROKER_PASSWORD: &str = "FAN_CONTROL_MQTT_BROKER_PASSWORD";
    const MQTT_BROKER_ADDRESS: &str = "FAN_CONTROL_MQTT_BROKER_ADDRESS";
    const MQTT_BROKER_PORT: &str = "FAN_CONTROL_MQTT_BROKER_PORT";

    dotenvy::dotenv()?;
    let wifi_network =
        env::var(WIFI_NETWORK).map_err(|error| BuildError::MissingEnvVar(error, WIFI_NETWORK))?;
    let wifi_password =
        env::var(WIFI_PASSWORD).map_err(|error| BuildError::MissingEnvVar(error, WIFI_PASSWORD))?;

    let mqtt_broker_username = env::var(MQTT_BROKER_USERNAME)
        .map_err(|error| BuildError::MissingEnvVar(error, MQTT_BROKER_USERNAME))?;
    let mqtt_broker_password = env::var(MQTT_BROKER_PASSWORD)
        .map_err(|error| BuildError::MissingEnvVar(error, MQTT_BROKER_PASSWORD))?;
    let mqtt_broker_address = env::var(MQTT_BROKER_ADDRESS)
        .map_err(|error| BuildError::MissingEnvVar(error, MQTT_BROKER_ADDRESS))?;
    let mqtt_broker_port = env::var(MQTT_BROKER_PORT)
        .map_err(|error| BuildError::MissingEnvVar(error, MQTT_BROKER_PORT))?;

    println!("cargo:rustc-env=FAN_CONTROL_WIFI_NETWORK={wifi_network}");
    println!("cargo:rustc-env=FAN_CONTROL_WIFI_PASSWORD={wifi_password}");
    println!("cargo:rustc-env=FAN_CONTROL_MQTT_BROKER_USERNAME={mqtt_broker_username}");
    println!("cargo:rustc-env=FAN_CONTROL_MQTT_BROKER_PASSWORD={mqtt_broker_password}");
    println!("cargo:rustc-env=FAN_CONTROL_MQTT_BROKER_ADDRESS={mqtt_broker_address}");
    println!("cargo:rustc-env=FAN_CONTROL_MQTT_BROKER_PORT={mqtt_broker_port}");

    Ok(())
}

fn extract_abreviations() -> HashMap<Rc<str>, Rc<str>> {
    const PATH: &str = "home-assistant/core/homeassistant/components/mqtt/abbreviations.py";
    println!("cargo::rerun-if-changed={PATH}");
    let module = fs::read_to_string(PATH).unwrap();
    let module = ruff_python_parser::parse_module(&module).unwrap();
    let syntax = module.syntax();

    let mut abbreviations = HashMap::new();
    for statement in &syntax.body {
        //TODO can I remove clone-clowning?
        let Some(assignment) = statement.clone().assign_stmt() else {
            continue;
        };

        let Some(name_expression) = assignment
            .targets
            .first()
            .cloned()
            .and_then(|expression| expression.name_expr())
        else {
            continue;
        };

        if name_expression.id.as_str() != "ABBREVIATIONS" {
            continue;
        }

        let Some(dictionary) = assignment.value.dict_expr() else {
            continue;
        };

        for item in dictionary.items {
            let Some(key) = item.key else {
                todo!("Expected abbreviations dictionary to have a key")
            };

            let Some(key_string_literal) = key.string_literal_expr() else {
                todo!("Expected key to be a string literal")
            };

            let key = key_string_literal.value.to_str();

            let Some(value_string_literal) = item.value.string_literal_expr() else {
                todo!("Expected value to be a string literal")
            };

            let value = value_string_literal.value.to_str();
            abbreviations.insert(Rc::from(value), Rc::from(key));
        }

        // Early return as we have what we want
        return abbreviations;
    }

    abbreviations
}

/// Overengineering saving a couple of bytes from a JSON string.
/// Minimizes payload with abbreviations and removing whitespace.
/// Abbreviations are loaded from the official home assistant repository.
/// TODO: device and origin abbreviations
fn setup_discovery_payload() -> Result<(), DiscoveryPayloadError> {
    const PATH_DISCOVER_JSON: &str = "discovery_payload.json";
    //TODO validate discovery payload
    let json = fs::read_to_string(PATH_DISCOVER_JSON)
        .map_err(|error| DiscoveryPayloadError::ReadDiscoveryPayload(error, PATH_DISCOVER_JSON))?;

    let mut is_in_string = false;
    let mut result = String::new();
    let abbreviations = extract_abreviations();
    let mut current_key = String::new();
    let mut is_in_key = true;
    for character in json.chars() {
        // Do not trim strings
        if character == '"' {
            // End of key
            if is_in_string && is_in_key {
                let key = Rc::from(current_key.as_ref());
                let abbreviation = abbreviations.get(&key);
                if let Some(abbreviation) = abbreviation {
                    println!("Abbreviation {key} -> {abbreviation}");
                    result.push_str(abbreviation);
                } else {
                    result.push_str(&current_key);
                }
                current_key.clear();
                result.push('"');
                is_in_string = false;
                is_in_key = false;
                continue;
            }

            is_in_string = !is_in_string;
            result.push('"');
            continue;
        }

        // End of value
        if character == ',' || character == '}' {
            is_in_key = true;
            result.push(',');
            continue;
        }

        if !is_in_string && character.is_whitespace() {
            continue;
        }

        if is_in_key {
            current_key.push(character);
            continue;
        }

        result.push(character);
    }

    println!("cargo:rustc-env=FAN_CONTROLLER_DISCOVERY_PAYLOAD={result}");

    Ok(())
}

fn get_git_hash() -> Result<String, GitHashError> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map_err(GitHashError::CommandExecution)?;

    let hash = String::from_utf8(output.stdout).map_err(GitHashError::ParseHash)?;

    Ok(hash)
}

fn set_discovery_payload(git_hash: &str) {
    let package_version = env!("CARGO_PKG_VERSION");
    // Following semantic versioning build metadata
    let version: Option<Rc<str>> = Option::from(Rc::from(format!("{package_version}+{git_hash}")));
    println!("Setting version to {version:?}");
    let payload = DiscoveryPayload {
        device: Device {
            identifiers: Some(ListOrString::String(topic::fan_controller::OBJECT_ID)),
            name: Some("Fan Controller"),
            model: Some("Raspberry Pi Pico W 1"),
            manufacturer: Some("claas.dev"),
            hardware_version: Some("1.0"),
            software_version: version.clone(),
            ..Default::default()
        },
        origin: Origin {
            name: "fan-controller",
            software_version: version,
            support_url: Some("https://github.com/SantaClaas/embedded-fan-control"),
        },
        components: HashMap::from([
            // Fan 1
            (
                "fan-1".to_string(),
                Component::Fan {
                    name: Some("Fan 1"),
                    unique_id: Some(topic::fan_controller::fan_1::UNIQUE_ID),
                    state_topic: Some(topic::fan_controller::fan_1::STATE),
                    command_topic: topic::fan_controller::fan_1::COMMAND,
                    percentage_state_topic: Some(topic::fan_controller::fan_1::percentage::STATE),
                    percentage_command_topic: Some(
                        topic::fan_controller::fan_1::percentage::COMMAND,
                    ),
                    speed_range_max: Some(32_000),
                },
            ),
            // Fan 2
            (
                "fan-2".to_string(),
                Component::Fan {
                    name: Some("Fan 2"),
                    unique_id: Some(topic::fan_controller::fan_2::UNIQUE_ID),
                    state_topic: Some(topic::fan_controller::fan_2::STATE),
                    command_topic: topic::fan_controller::fan_2::COMMAND,
                    percentage_state_topic: Some(topic::fan_controller::fan_2::percentage::STATE),
                    percentage_command_topic: Some(
                        topic::fan_controller::fan_2::percentage::COMMAND,
                    ),
                    speed_range_max: Some(32_000),
                },
            ),
        ]),
        quality_of_service: None,
        state_topic: Some(topic::fan_controller::STATE),
        command_topic: Some(topic::fan_controller::COMMAND),
        encoding: None,
    };

    let payload = serde_json::to_string(&payload).unwrap();

    println!("cargo:rustc-env=FAN_CONTROLLER_DISCOVERY_PAYLOAD={payload}",);
}

fn main() -> Result<(), BuildError> {
    ensure_memory_x_file()?;
    setup_configuration()?;
    // setup_discovery_payload()?;
    let git_hash = get_git_hash()?;
    set_discovery_payload(&git_hash);
    Ok(())
}
