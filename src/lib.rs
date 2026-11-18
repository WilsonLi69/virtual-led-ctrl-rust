//! Virtual LED controller for OpenBMC.
//!
//! A Rust re-implementation of the `virtual-led-ctrl` daemon: it drives one
//! GPIO line and exposes it over D-Bus using the
//! `xyz.openbmc_project.Led.Physical` interface.
//!
//! The crate is split so that the D-Bus and blink logic can be tested without
//! real GPIO hardware:
//!
//! * [`led`] owns the GPIO line and the blink scheduler.
//! * [`dbus`] maps the `xyz.openbmc_project.Led.Physical` properties onto the
//!   shared [`led::Config`].

pub mod dbus;
pub mod led;

/// Well-known bus name owned by the daemon.
pub const SERVICE_NAME: &str = "xyz.openbmc_project.LED.Controller.VirtualTest";

/// Object path of the virtual LED.
pub const LED_OBJECT_PATH: &str = "/xyz/openbmc_project/led/physical/virtual_test";
