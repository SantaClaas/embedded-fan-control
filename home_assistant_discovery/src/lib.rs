//! # Home Assistant Discovery Library
//! Tools and utilities for creating the discovery payload for MQTT Home Assistant devices during build time for embedded systems.

use std::{collections::HashMap, rc::Rc};

use mqtt::QualityOfService;
use serde::Serialize;

/// Utility type for when the Home Assistant type definition allows either a list or a string.
#[derive(Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum ListOrString {
    List(Vec<String>),
    String(&'static str),
}

/// Information about the device this fan is a part of to tie it into the device registry. Only works when unique_id is set. At least one of identifiers or connections must be present to identify the device.
#[derive(Serialize, Default)]
pub struct Device {
    /// A list of IDs that uniquely identify the device. For example a serial number.
    #[serde(rename = "ids")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifiers: Option<ListOrString>,
    /// The name of the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'static str>,
    /// The model of the device.
    #[serde(rename = "mdl")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<&'static str>,
    /// The manufacturer of the device.
    #[serde(rename = "mf")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<&'static str>,
    /// The hardware version of the device.
    #[serde(rename = "hw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hardware_version: Option<&'static str>,
    /// The firmware version of the device.
    #[serde(rename = "sw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<Rc<str>>,
}

#[derive(Serialize, Default)]
pub struct Origin {
    /// The name of the application that is the origin of the discovered MQTT item. (Required)
    pub name: &'static str,
    /// Software version of the application that supplies the discovered MQTT item.
    #[serde(rename = "sw")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<Rc<str>>,
    /// Support URL of the application that supplies the discovered MQTT item.
    #[serde(rename = "url")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub support_url: Option<&'static str>,
}

/// Internally tagged by the required `platform` (`p`) field
#[derive(Serialize)]
#[serde(tag = "p")]
pub enum Component {
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
pub struct DiscoveryPayload {
    #[serde(rename = "dev")]
    pub device: Device,
    #[serde(rename = "o")]
    pub origin: Origin,
    #[serde(rename = "cmps")]
    pub components: HashMap<String, Component>,
    #[serde(rename = "qos")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality_of_service: Option<QualityOfService>,
    #[serde(rename = "stat_t")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state_topic: Option<&'static str>,
    #[serde(rename = "cmd_t")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_topic: Option<&'static str>,
    /// The encoding of the payloads received and published messages. Set to "" to disable decoding of incoming payload.
    /// Default is "utf-8"
    #[serde(rename = "e")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_home_assistant_example() {}

    #[test]
    fn serialize_custom_example() {
        let version = Some(Rc::from("test-version"));
        let payload = DiscoveryPayload {
            device: Device {
                identifiers: Some(ListOrString::String("fancontroller-device")),
                name: Some("Fan Controller"),
                model: Some("Raspberry Pi Pico W 1"),
                manufacturer: Some("claas.dev"),
                hardware_version: Some("1.0"),
                software_version: version.clone(),
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
                        unique_id: Some("fancontroller/fan-1"),
                        state_topic: Some(topic::fan_controller::fan_1::STATE),
                        command_topic: topic::fan_controller::fan_1::COMMAND,
                        percentage_state_topic: Some(
                            topic::fan_controller::fan_1::percentage::STATE,
                        ),
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
                        percentage_state_topic: Some(
                            topic::fan_controller::fan_2::percentage::STATE,
                        ),
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

        insta::assert_json_snapshot!(payload)
    }
}
