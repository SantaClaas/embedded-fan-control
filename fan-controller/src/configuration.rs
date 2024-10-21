use embassy_net::IpAddress;
use embassy_time::Duration;

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_NETWORK: &str = env!("FAN_CONTROL_WIFI_NETWORK");

//TODO make configurable
/// Don't put credentials in the source code
pub(crate) const WIFI_PASSWORD: &str = env!("FAN_CONTROL_WIFI_PASSWORD");

//TODO make configurable
pub(crate) const MQTT_BROKER_ADDRESS: &str = "homeassistant";

//TODO make configurable
pub(crate) const MQTT_BROKER_PORT: u16 = 1883;

//TODO make configurable
/// The broker IP address can be configured manually. It will be used instead of the [MQTT_BROKER_ADDRESS]
/// if it is set as it does not require DNS resolution.
pub(crate) const MQTT_BROKER_IP_ADDRESS: Option<IpAddress> = None;

//TODO make configurable
/// Set in Homeassitant under Settings > People > Users Tab. Not to be confused with the People tab.
/// The Users Tab might only be visible in advanced mode as administrator.
/// A separate account is recommended for each device.
pub(crate) struct MqttBrokerCredentials<'a> {
    pub(crate) username: &'a str,
    pub(crate) password: &'a [u8],
}

pub(crate) const MQTT_BROKER_CREDENTIALS: MqttBrokerCredentials = MqttBrokerCredentials {
    username: "fancontroller",
    password: b"test",
};

//TODO make configurable
/// Prefix is "homeassistant", but it can be changed in home assistant configuration
pub(crate) const DISCOVERY_TOPIC: &str = "homeassistant/fan/testfan/config";

/// The keep alive interval defines the maximum time between messages sent to the broker.
/// The broker will disconnect the client if no message is received within 1.5 times of the keep alive interval.
pub(crate) const KEEP_ALIVE: Duration = Duration::from_secs(10);
const _: () = {
    // Check if the representation as u16 is correct
    let seconds = KEEP_ALIVE.as_secs() as u16;

    core::assert!(seconds == 10);
};

/// The timeout not to be confused with the keep alive interval is used for packets that require a
/// response packet from the broker. If the client does not receive a response within the timeout
/// the client will stop waiting for a response which can lead to a disconnect or retry in some cases.
pub(crate) const MQTT_TIMEOUT: Duration = Duration::from_secs(60);

/// The timeout to wait for a response from the fans before cancelling waiting for a response.
pub(crate) const FAN_TIMEOUT: Duration = Duration::from_secs(5);
