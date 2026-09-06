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

    /// Why the controller last reset, published retained on boot. Retained because the resets
    /// worth diagnosing are rare and nobody is subscribed when they happen — the answer has to
    /// still be there whenever someone next looks.
    pub const RESET_CAUSE: &str = formatcp!("{OBJECT_ID}/reset-cause");

    /// The topic to publish the on/off state of the fan controller.
    pub const STATE: &str = formatcp!("{OBJECT_ID}/on/state");
    /// The topic to subscribe to for setting the on/off state of the fan controller.
    /// This topic is used by Home Assistant to notify the fan controller to turn on or off.
    pub const COMMAND: &str = formatcp!("{OBJECT_ID}/on/set");

    /// The relay module on the controller's second Modbus bus.
    ///
    /// It has one contact and nothing else — no speed, nothing it measures — so unlike a fan it is
    /// a plain on/off pair of topics
    pub mod relay {
        use super::OBJECT_ID;
        use const_format::formatcp;

        pub const UNIQUE_ID: &str = formatcp!("{OBJECT_ID}/relay-1");

        pub mod state {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            /// Published after the module has confirmed the coil write, never before it
            pub const STATE: &str = formatcp!("{UNIQUE_ID}/on/state");
            /// Subscribed to for Home Assistant asking the contact to open or close
            pub const COMMAND: &str = formatcp!("{UNIQUE_ID}/on/set");
        }
    }

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

        /// Every sensor value a fan reports arrives as one JSON object on this topic, so a poll
        /// costs a single publish and Home Assistant picks each value out with a value template
        pub mod sensor {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/sensors/state");

            /// The identifiers Home Assistant tells the sensors apart by. They are not
            /// topics, but they are composed from the same fan identifier and have to stay unique
            /// alongside it, so they belong next to it rather than in the build script
            pub const SPEED: &str = formatcp!("{UNIQUE_ID}/sensors/speed");
            pub const MOTOR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/motor-temperature");
            pub const ELECTRONICS_TEMPERATURE: &str =
                formatcp!("{UNIQUE_ID}/sensors/electronics-temperature");
            pub const POWER: &str = formatcp!("{UNIQUE_ID}/sensors/power");
            /// The air the fan is moving rather than the fan itself: the first two come from the
            /// temperature/humidity sensor wired to it, the third from what the fan measures of
            /// the flow through it
            pub const AIR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/air-temperature");
            pub const AIR_HUMIDITY: &str = formatcp!("{UNIQUE_ID}/sensors/air-humidity");
            pub const VOLUME_FLOW: &str = formatcp!("{UNIQUE_ID}/sensors/volume-flow");
            /// What the fan says is wrong with it, and what it says is close to going wrong.
            /// Read out of the same status run as the speed and the temperatures
            pub const MOTOR_STATUS: &str = formatcp!("{UNIQUE_ID}/sensors/motor-status");
            pub const WARNING: &str = formatcp!("{UNIQUE_ID}/sensors/warning");
            /// The analog set point input's own entity. It is always broken here, because the set
            /// point comes over RS-485 and nothing is wired to that input, so it is kept out of
            /// the warning above rather than sitting in it permanently
            pub const ANALOG_SET_POINT: &str = formatcp!("{UNIQUE_ID}/sensors/analog-set-point");
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

        /// Every sensor value a fan reports arrives as one JSON object on this topic, so a poll
        /// costs a single publish and Home Assistant picks each value out with a value template
        pub mod sensor {
            use super::UNIQUE_ID;
            use const_format::formatcp;

            pub const STATE: &str = formatcp!("{UNIQUE_ID}/sensors/state");

            /// The identifiers Home Assistant tells the sensors apart by. They are not
            /// topics, but they are composed from the same fan identifier and have to stay unique
            /// alongside it, so they belong next to it rather than in the build script
            pub const SPEED: &str = formatcp!("{UNIQUE_ID}/sensors/speed");
            pub const MOTOR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/motor-temperature");
            pub const ELECTRONICS_TEMPERATURE: &str =
                formatcp!("{UNIQUE_ID}/sensors/electronics-temperature");
            pub const POWER: &str = formatcp!("{UNIQUE_ID}/sensors/power");
            /// The air the fan is moving rather than the fan itself: the first two come from the
            /// temperature/humidity sensor wired to it, the third from what the fan measures of
            /// the flow through it
            pub const AIR_TEMPERATURE: &str = formatcp!("{UNIQUE_ID}/sensors/air-temperature");
            pub const AIR_HUMIDITY: &str = formatcp!("{UNIQUE_ID}/sensors/air-humidity");
            pub const VOLUME_FLOW: &str = formatcp!("{UNIQUE_ID}/sensors/volume-flow");
            /// What the fan says is wrong with it, and what it says is close to going wrong.
            /// Read out of the same status run as the speed and the temperatures
            pub const MOTOR_STATUS: &str = formatcp!("{UNIQUE_ID}/sensors/motor-status");
            pub const WARNING: &str = formatcp!("{UNIQUE_ID}/sensors/warning");
            /// The analog set point input's own entity. It is always broken here, because the set
            /// point comes over RS-485 and nothing is wired to that input, so it is kept out of
            /// the warning above rather than sitting in it permanently
            pub const ANALOG_SET_POINT: &str = formatcp!("{UNIQUE_ID}/sensors/analog-set-point");
        }
    }
}
