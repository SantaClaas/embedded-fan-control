#![no_std]

pub mod fan_controller {
    /// The topic to publish the on/off state of the fan controller.
    pub const STATE: &str = "fancontroller/on/state";
    /// The topic to subscribe to for setting the on/off state of the fan controller.
    /// This topic is used by Home Assistant to notify the fan controller to turn on or off.
    pub const COMMAND: &str = "fancontroller/on/set";

    pub mod fan_1 {
        /// The topic to publish the on/off state of the fan 1 on the fan controller.
        pub const STATE: &str = "fancontroller/fan-1/on/state";
        /// The topic to subscribe to for setting the on/off state of the fan 1 on the fan controller.
        /// This topic is used by Home Assistant to notify the fan controller to turn on or off the fan.
        pub const COMMAND: &str = "fancontroller/fan-1/on/set";

        pub mod percentage {
            pub const STATE: &str = "fancontroller/fan-2/speed/percentage_state";
            pub const COMMAND: &str = "fancontroller/fan-2/speed/percentage";
        }
    }

    pub mod fan_2 {
        /// The topic to publish the on/off state of the fan 2 on the fan controller.
        pub const STATE: &str = "fancontroller/fan-2/on/state";
        /// The topic to subscribe to for setting the on/off state of the fan 2 on the fan controller.
        /// This topic is used by Home Assistant to notify the fan controller to turn on or off the fan.
        pub const COMMAND: &str = "fancontroller/fan-2/on/set";

        pub mod percentage {
            pub const STATE: &str = "fancontroller/fan-2/speed/percentage_state";
            pub const COMMAND: &str = "fancontroller/fan-2/speed/percentage";
        }
    }
}
