#![no_std]

pub mod fan_controller {
    use const_format::formatcp;

    pub const OBJECT_ID: &str = "fan-controller";
    /// Prefix is "homeassistant", but it can be changed in home assistant configuration
    const DISCOVERY_PREFIX: &str = "homeassistant";
    /// One of the supported MQTT integrations, e.g., binary_sensor, or device in case of a device discovery.
    /// Must be set to "device" when a device exposes multiple components in one payload
    const COMPONENT: &str = "device";

    pub const DISCOVERY: &str = formatcp!("{DISCOVERY_PREFIX}/{COMPONENT}/{OBJECT_ID}/config");

    /// The topic to publish the on/off state of the fan controller.
    pub const STATE: &str = formatcp!("{OBJECT_ID}/on/state");
    /// The topic to subscribe to for setting the on/off state of the fan controller.
    /// This topic is used by Home Assistant to notify the fan controller to turn on or off.
    pub const COMMAND: &str = formatcp!("{OBJECT_ID}/on/set");

    pub mod fan_1 {
        use super::OBJECT_ID;
        use const_format::formatcp;

        pub const UNIQUE_ID: &str = formatcp!("{OBJECT_ID}/fan-1");
        pub mod on {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            /// The topic to publish the on/off state of the fan 1 on the fan controller.
            pub const STATE: &str = formatcp!("{UNIQUE_ID}/on/state");
            /// The topic to subscribe to for setting the on/off state of the fan 1 on the fan controller.
            /// This topic is used by Home Assistant to notify the fan controller to turn on or off the fan.
            pub const COMMAND: &str = formatcp!("{UNIQUE_ID}/on/set");
        }

        pub mod percentage {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/speed/percentage_state");
            pub const COMMAND: &str = formatcp!("{UNIQUE_ID}/speed/percentage");
        }
    }

    pub mod fan_2 {
        use super::OBJECT_ID;
        use const_format::formatcp;

        pub const UNIQUE_ID: &str = formatcp!("{OBJECT_ID}/fan-2");

        pub mod on {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            /// The topic to publish the on/off state of the fan 2 on the fan controller.
            pub const STATE: &str = formatcp!("{UNIQUE_ID}/on/state");
            /// The topic to subscribe to for setting the on/off state of the fan 2 on the fan controller.
            /// This topic is used by Home Assistant to notify the fan controller to turn on or off the fan.
            pub const COMMAND: &str = formatcp!("{UNIQUE_ID}/on/set");
        }

        pub mod percentage {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/speed/percentage_state");
            pub const COMMAND: &str = formatcp!("{UNIQUE_ID}/speed/percentage");
        }
    }
}
