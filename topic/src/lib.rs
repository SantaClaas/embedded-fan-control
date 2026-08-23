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
        /// The on and off state command and state topics for fan 1.
        pub mod state {
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

        /// All five sensor values a fan reports arrive as one JSON object on this topic, so a poll
        /// costs a single publish and Home Assistant picks each value out with a value template
        pub mod sensors {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/sensors/state");

            /// The identifiers Home Assistant tells the five sensors apart by. They are not
            /// topics, but they are composed from the same fan identifier and have to stay unique
            /// alongside it, so they belong next to it rather than in the build script
            pub const SPEED: &str = formatcp!("{UNIQUE_ID}/sensors/speed");
            pub const MOTOR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/motor-temperature");
            pub const ELECTRONICS_TEMPERATURE: &str =
                formatcp!("{UNIQUE_ID}/sensors/electronics-temperature");
            pub const POWER: &str = formatcp!("{UNIQUE_ID}/sensors/power");
            pub const ENERGY: &str = formatcp!("{UNIQUE_ID}/sensors/energy");
        }
    }

    pub mod fan_2 {
        use super::OBJECT_ID;
        use const_format::formatcp;

        pub const UNIQUE_ID: &str = formatcp!("{OBJECT_ID}/fan-2");

        /// The on and off state command and state topics for fan 2.
        pub mod state {
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

        /// All five sensor values a fan reports arrive as one JSON object on this topic, so a poll
        /// costs a single publish and Home Assistant picks each value out with a value template
        pub mod sensors {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/sensors/state");

            /// The identifiers Home Assistant tells the five sensors apart by. They are not
            /// topics, but they are composed from the same fan identifier and have to stay unique
            /// alongside it, so they belong next to it rather than in the build script
            pub const SPEED: &str = formatcp!("{UNIQUE_ID}/sensors/speed");
            pub const MOTOR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/motor-temperature");
            pub const ELECTRONICS_TEMPERATURE: &str =
                formatcp!("{UNIQUE_ID}/sensors/electronics-temperature");
            pub const POWER: &str = formatcp!("{UNIQUE_ID}/sensors/power");
            pub const ENERGY: &str = formatcp!("{UNIQUE_ID}/sensors/energy");
        }
    }
}
