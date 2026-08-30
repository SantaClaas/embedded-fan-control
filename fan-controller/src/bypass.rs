//! The bypass damper, which is driven by a relay module rather than by either fan.
//!
//! A summer bypass leads the incoming air around the heat exchanger, so the outgoing air stops
//! warming the fresh air on its way in. It has two positions and nothing in between, which is why
//! it is a relay and not another set point.
//!
//! Only the constants live here. The routine that drives it is in `main.rs` alongside the other
//! routines, the same way [`crate::fan`] holds a fan's addresses and `fan_control_routine` holds
//! what is done with them.
//!
//! The firmware's convention is that an energised relay means the bypass is open, and the wiring
//! has to match it. Which contact the damper hangs off is what decides where it ends up when the
//! relay loses power, which is not something the firmware can see. See the bypass section of
//! `documentation.md`.

use crate::modbus;

/// The relay's modbus address.
///
/// It has to be set on the module once before it goes on the bus. The address it ships with is
/// `0xFF`, which modbus reserves rather than gives to a device, and `0x04` carries on from the
/// fans at `0x02` and `0x03`.
/// See Alssay single-way Modbus relay module LC-Modbus-1R-D7, section 3, instruction 5
pub(crate) const ADDRESS: modbus::device::Address = modbus::device::Address::new(0x04);

/// The module carries eight coil addresses but has only one relay, which is the first of them.
///
/// Not to be confused with holding register `0x0000` on the same device, which is the module's own
/// address — writing the bypass position there would renumber the module instead of moving the
/// damper. That is why a coil address is its own type
pub(crate) const COIL: modbus::coil::Address = modbus::coil::Address::new(0x0000);
