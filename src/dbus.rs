//! `xyz.openbmc_project.Led.Physical` D-Bus interface.

use tokio::sync::watch;
use zbus::fdo;
use zbus::interface;

use crate::led::{Action, Config};

/// D-Bus view of the virtual LED.
///
/// The properties live in a watch channel shared with the blink task started by
/// [`crate::led::run`], so every property write is applied to the GPIO line
/// immediately. zbus emits `PropertiesChanged` for each accepted write.
pub struct PhysicalLed {
    config: watch::Sender<Config>,
}

impl PhysicalLed {
    /// Creates the interface on top of the shared LED configuration.
    pub fn new(config: watch::Sender<Config>) -> Self {
        Self { config }
    }
}

#[interface(name = "xyz.openbmc_project.Led.Physical")]
impl PhysicalLed {
    /// Current state of the LED.
    #[zbus(property)]
    fn state(&self) -> String {
        self.config.borrow().action.as_str().to_owned()
    }

    #[zbus(property)]
    fn set_state(&mut self, state: &str) -> fdo::Result<()> {
        let action = Action::from_dbus(state)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("unknown LED state: {state}")))?;
        self.config.send_modify(|config| config.action = action);
        Ok(())
    }

    /// Blink period in milliseconds for one on/off cycle.
    #[zbus(property)]
    fn period(&self) -> u16 {
        self.config.borrow().period_ms
    }

    #[zbus(property)]
    fn set_period(&mut self, period: u16) -> fdo::Result<()> {
        if period == 0 {
            return Err(fdo::Error::InvalidArgs("period must not be zero".into()));
        }
        self.config.send_modify(|config| config.period_ms = period);
        Ok(())
    }

    /// Percentage of the blink period the LED stays on.
    #[zbus(property)]
    fn duty_on(&self) -> u8 {
        self.config.borrow().duty_on
    }

    #[zbus(property)]
    fn set_duty_on(&mut self, duty_on: u8) -> fdo::Result<()> {
        if duty_on > 100 {
            return Err(fdo::Error::InvalidArgs(format!(
                "duty cycle must be a percentage (0-100), got {duty_on}"
            )));
        }
        self.config.send_modify(|config| config.duty_on = duty_on);
        Ok(())
    }
}
