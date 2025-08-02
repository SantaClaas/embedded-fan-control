use crate::{task::MqttBrokerCredentials, MqttBrokerConfiguration};
use embassy_net::IpAddress;
use embassy_time::Duration;

/// Thanks!
/// - https://users.rust-lang.org/t/pass-numeric-compile-time-arguments/59093/2
/// - https://www.reddit.com/r/rust/comments/10ol38k/comment/j6jrpvd
const fn parse_u16(string: &str) -> u16 {
    let mut out: u16 = 0;
    let mut index: usize = 0;
    while index < string.len() {
        out *= 10;
        let byte = string.as_bytes()[index];
        assert!(
            b'0' <= byte && byte <= b'9',
            "invalid digit. Needs to be a number",
        );
        out += (byte - b'0') as u16;
        index += 1;
    }
    out
}

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_NETWORK: &str = env!("FAN_CONTROL_WIFI_NETWORK");

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_PASSWORD: &str = env!("FAN_CONTROL_WIFI_PASSWORD");

//TODO make configurable
pub(crate) const MQTT_BROKER_ADDRESS: &str = env!("FAN_CONTROL_MQTT_BROKER_ADDRESS");

//TODO make configurable
pub(crate) const MQTT_BROKER_PORT: u16 = parse_u16(env!("FAN_CONTROL_MQTT_BROKER_PORT"));

//TODO make configurable
/// The broker IP address can be configured manually. It will be used instead of the [MQTT_BROKER_ADDRESS]
/// if it is set as it does not require DNS resolution.
pub(crate) const MQTT_BROKER_IP_ADDRESS: Option<IpAddress> = None;

pub(crate) const MQTT_BROKER_CREDENTIALS: MqttBrokerCredentials = MqttBrokerCredentials {
    username: env!("FAN_CONTROL_MQTT_BROKER_USERNAME"),
    password: env!("FAN_CONTROL_MQTT_BROKER_PASSWORD").as_bytes(),
};

//TODO make configurable
/// Prefix is "homeassistant", but it can be changed in home assistant configuration
pub(crate) const DISCOVERY_TOPIC: &str = "homeassistant/fan/fancontroller/config";

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
pub(crate) const MQTT_TIMEOUT: Duration = Duration::from_secs(60);

/// The timeout to wait for a response from the fans before cancelling waiting for a response.
pub(crate) const FAN_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) const MQTT_BROKER: MqttBrokerConfiguration<'_> = MqttBrokerConfiguration {
    address: MQTT_BROKER_ADDRESS,
    credentials: MQTT_BROKER_CREDENTIALS,
    client_identifier: env!("CARGO_PKG_NAME"),
    keep_alive_seconds: KEEP_ALIVE,
};
