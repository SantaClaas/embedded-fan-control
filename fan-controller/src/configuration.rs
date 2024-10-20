use embassy_net::IpAddress;
use embassy_time::Duration;

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_NETWORK: &str = ""; //  env!("FAN_CONTROL_WIFI_NETWORK");

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_PASSWORD: &str = ""; //env!("FAN_CONTROL_WIFI_PASSWORD");

//TODO make configurable
pub(crate) const MQTT_BROKER_ADDRESS: &str = "homeassistant.local";

//TODO make configurable
pub(crate) const MQTT_BROKER_PORT: u16 = 1883;

//TODO make configurable
/// The broker IP address can be configured manually. It will be used instead of the [MQTT_BROKER_ADDRESS]
/// if it is set as it does not require DNS resolution.
pub(crate) const MQTT_BROKER_IP_ADDRESS: Option<IpAddress> = None;

//TODO make configurable
pub(crate) const MQTT_BROKER_USERNAME: &str = "";

//TODO make configurable
pub(crate) const MQTT_BROKER_PASSWORD: &[u8] = b"";

//TODO make configurable
/// Prefix is "homeassistant", but it can be changed in home assistant configuration
pub(crate) const DISCOVERY_TOPIC: &str = "homeassistant/fan/testfan/config";

/// The keep alive interval defines the maximum time between messages sent to the broker.
/// The broker will disconnect the client if no message is received within 1.5 times of the keep alive interval.
pub(crate) const KEEP_ALIVE: Duration = Duration::from_secs(60);
const _: () = {
    // Check if the representation as u16 is correct
    let seconds = KEEP_ALIVE.as_secs() as u16;

    core::assert!(seconds == 60);
};

/// The timeout not to be confused with the keep alive interval is used for packets that require a
/// response packet from the broker. If the client does not receive a response within the timeout
/// the client will stop waiting for a response which can lead to a disconnect or retry in some cases.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(5);