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
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;

use mqtt::QualityOfService;
use ruff_python_ast::{DictItem, Expr, Stmt};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
enum ListOrString {
    List(Vec<String>),
    String(&'static str),
}

/// Information about the device this fan is a part of to tie it into the device registry. Only works when unique_id is set. At least one of identifiers or connections must be present to identify the device.
#[derive(Serialize, Default)]
struct Device {
    /// A list of IDs that uniquely identify the device. For example a serial number.
    #[serde(rename = "ids")]
    #[serde(skip_serializing_if = "Option::is_none")]
    identifiers: Option<ListOrString>,
    /// The name of the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'static str>,
    /// The model of the device.
    #[serde(rename = "mdl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'static str>,
    /// The manufacturer of the device.
    #[serde(rename = "mf")]
    #[serde(skip_serializing_if = "Option::is_none")]
    manufacturer: Option<&'static str>,
    /// The hardware version of the device.
    #[serde(rename = "hw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    hardware_version: Option<&'static str>,
    /// The firmware version of the device.
    #[serde(rename = "sw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    software_version: Option<&'static str>,
}

#[derive(Serialize, Default)]
struct Origin {
    /// The name of the application that is the origin of the discovered MQTT item. (Required)
    name: &'static str,
    /// Software version of the application that supplies the discovered MQTT item.
    #[serde(rename = "sw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    software_version: Option<&'static str>,
    /// Support URL of the application that supplies the discovered MQTT item.
    #[serde(rename = "url")]
    #[serde(skip_serializing_if = "Option::is_none")]
    support_url: Option<&'static str>,
}

/// Internally tagged by the required `platform` (`p`) field
#[derive(Serialize)]
#[serde(tag = "p")]
enum Component {
    Fan {
        /// The name of the fan. Can be set to null if only the device name is relevant.
        name: Option<&'static str>,
        /// An ID that uniquely identifies this fan. If two fans have the same unique ID, Home Assistant will raise an exception. Required when used with device-based discovery.
        #[serde(rename = "uniq_id")]
        #[serde(skip_serializing_if = "Option::is_none")]
        unique_id: Option<&'static str>,
        /// The MQTT topic subscribed to receive state updates. A “None” payload resets to an unknown state. An empty payload is ignored. By default, valid state payloads are OFF and ON. The accepted payloads can be overridden with the payload_off and payload_on config options.
        #[serde(rename = "stat_t")]
        #[serde(skip_serializing_if = "Option::is_none")]
        state_topic: Option<&'static str>,

        /// The MQTT topic to publish commands to change the fan state.
        #[serde(rename = "cmd_t")]
        command_topic: &'static str,
        /// The MQTT topic subscribed to receive fan speed based on percentage.
        #[serde(rename = "pct_stat_t")]
        percentage_state_topic: Option<&'static str>,
        /// The MQTT topic to publish commands to change the fan speed state based on a percentage.
        #[serde(rename = "pct_cmd_t")]
        percentage_command_topic: Option<&'static str>,
        /// The maximum of numeric output range (representing 100 %). The percentage_step is defined by 100 / the number of speeds within the speed range.
        /// Default: 100
        #[serde(rename = "spd_rng_max")]
        speed_range_max: Option<u16>,
    },
}

/// Home Assistant MQTT device-based Discovery Payload
/// This is for the multi [device discovery payload](https://www.home-assistant.io/integrations/mqtt/#device-discovery-payload).
/// It requires
/// - device
/// - origin
#[derive(Serialize, Default)]
struct DiscoveryPayload {
    #[serde(rename = "dev")]
    device: Device,
    #[serde(rename = "o")]
    origin: Origin,
    #[serde(rename = "cmps")]
    components: HashMap<String, Component>,
    #[serde(rename = "qos")]
    #[serde(skip_serializing_if = "Option::is_none")]
    quality_of_service: Option<QualityOfService>,
    #[serde(rename = "stat_t")]
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<&'static str>,
    #[serde(rename = "cmd_t")]
    #[serde(skip_serializing_if = "Option::is_none")]
    command_topic: Option<&'static str>,
    /// The encoding of the payloads received and published messages. Set to "" to disable decoding of incoming payload.
    /// Default is "utf-8"
    #[serde(rename = "e")]
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
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
    let module = fs::read_to_string(&PATH).unwrap();
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

    return abbreviations;
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
                    println!("Abbreviation {} -> {}", key, abbreviation);
                    result.push_str(&abbreviation);
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

    println!(
        "cargo:rustc-env=FAN_CONTROLLER_DISCOVERY_PAYLOAD={}",
        result
    );

    Ok(())
}

fn set_discovery_payload() {
    // No way to const a HashMap
    let payload = DiscoveryPayload {
        device: Device {
            identifiers: Some(ListOrString::String("fancontroller-device")),
            name: Some("Fan Controller"),
            model: Some("Raspberry Pi Pico W 1"),
            manufacturer: Some("claas.dev"),
            hardware_version: Some("1.0"),
            software_version: Some(env!("CARGO_PKG_VERSION")),
        },
        origin: Origin {
            name: "fan-controller",
            software_version: Some(env!("CARGO_PKG_VERSION")),
            support_url: Some("https://github.com/SantaClaas/embedded-fan-control"),
        },
        components: HashMap::from([
            // Fan 1
            (
                "fan-1".to_string(),
                Component::Fan {
                    name: Some("Fan 1"),
                    unique_id: Some("fancontroller/fan-1"),
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
                    unique_id: Some("fancontroller/fan-2"),
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
    set_discovery_payload();
    Ok(())
}
