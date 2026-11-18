//! GPIO output and blink scheduling.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use gpiocdev::line::Value;
use gpiocdev::Request;
use tokio::sync::watch;

/// D-Bus values of the `xyz.openbmc_project.Led.Physical.Action` enumeration.
const ACTION_ON: &str = "xyz.openbmc_project.Led.Physical.Action.On";
const ACTION_OFF: &str = "xyz.openbmc_project.Led.Physical.Action.Off";
const ACTION_BLINK: &str = "xyz.openbmc_project.Led.Physical.Action.Blink";

/// State of the LED, mirroring `xyz.openbmc_project.Led.Physical.Action`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Action {
    /// Solid on.
    On,
    /// Off.
    #[default]
    Off,
    /// Toggles with the configured period and duty cycle.
    Blink,
}

impl Action {
    /// D-Bus value of this action.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => ACTION_ON,
            Self::Off => ACTION_OFF,
            Self::Blink => ACTION_BLINK,
        }
    }

    /// Parses a D-Bus action value, returning `None` for unknown actions.
    pub fn from_dbus(value: &str) -> Option<Self> {
        match value {
            ACTION_ON => Some(Self::On),
            ACTION_OFF => Some(Self::Off),
            ACTION_BLINK => Some(Self::Blink),
            _ => None,
        }
    }
}

/// LED configuration, exposed as the `xyz.openbmc_project.Led.Physical`
/// properties.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Config {
    /// Current LED action.
    pub action: Action,
    /// Blink period in milliseconds for one on/off cycle.
    pub period_ms: u16,
    /// Percentage of the period the LED stays on while blinking.
    pub duty_on: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            action: Action::Off,
            period_ms: 1000,
            duty_on: 50,
        }
    }
}

impl Config {
    /// Time the LED stays on during one blink cycle.
    pub fn on_time(self) -> Duration {
        Duration::from_millis(self.scaled_period(self.duty_percent()))
    }

    /// Time the LED stays off during one blink cycle.
    pub fn off_time(self) -> Duration {
        Duration::from_millis(self.scaled_period(100 - self.duty_percent()))
    }

    /// Duty cycle clamped to the meaningful 0..=100 percentage range.
    fn duty_percent(self) -> u64 {
        u64::from(self.duty_on.min(100))
    }

    /// `percentage` of the period in milliseconds.
    ///
    /// A zero delay would turn the blink loop into a busy loop, so short
    /// cycles are rounded up to one millisecond.
    fn scaled_period(self, percentage: u64) -> u64 {
        (u64::from(self.period_ms) * percentage / 100).max(1)
    }
}

/// Sink for the LED output state.
///
/// [`GpioLed`] drives a real line; tests use an in-memory fake.
pub trait LedOutput: Send + Sync + 'static {
    /// Switches the LED on (`true`) or off (`false`).
    fn set(&self, on: bool);
}

/// LED output backed by a GPIO line.
pub struct GpioLed {
    request: Request,
}

impl GpioLed {
    /// Requests `line` on the GPIO chip called `chip` as an output, driven low
    /// by default.
    pub fn open(chip: &str, line: u32) -> Result<Self> {
        let chip_path = Path::new("/dev").join(chip);
        let request = Request::builder()
            .on_chip(chip_path.as_path())
            .with_line(line)
            .as_output(Value::Inactive)
            .with_consumer("Virtual-LED-Ctrl")
            .request()
            .with_context(|| format!("failed to request line {line} of {}", chip_path.display()))?;

        Ok(Self { request })
    }
}

impl LedOutput for GpioLed {
    fn set(&self, on: bool) {
        let value = if on { Value::Active } else { Value::Inactive };
        if let Err(err) = self.request.set_lone_value(value) {
            eprintln!("Failed to set LED output: {err}");
        }
    }
}

/// Drives `output` according to the configuration received on `config`.
///
/// Every change is applied immediately; while blinking, a change restarts the
/// cycle from the on phase. The task ends once the sender side of the watch
/// channel is dropped.
pub async fn run<O: LedOutput>(output: O, mut config: watch::Receiver<Config>) {
    loop {
        let cfg = *config.borrow_and_update();

        match cfg.action {
            Action::On => {
                println!("D-Bus request: ON -> Set GPIO High");
                output.set(true);
            }
            Action::Off => {
                println!("D-Bus request: OFF -> Set GPIO Low");
                output.set(false);
            }
            Action::Blink => {
                println!(
                    "D-Bus request: BLINK -> Period: {}ms, DutyOn: {}",
                    cfg.period_ms, cfg.duty_on
                );

                if cfg.duty_on >= 100 {
                    output.set(true);
                } else if cfg.duty_on == 0 {
                    output.set(false);
                } else {
                    if blink(&output, &mut config, cfg).await.is_err() {
                        return;
                    }
                    // A property changed: re-apply the new configuration.
                    continue;
                }
            }
        }

        if config.changed().await.is_err() {
            return;
        }
    }
}

/// Toggles `output` until the configuration changes.
async fn blink<O: LedOutput>(
    output: &O,
    config: &mut watch::Receiver<Config>,
    cfg: Config,
) -> Result<(), watch::error::RecvError> {
    let mut on = true;

    loop {
        output.set(on);
        let wait = if on { cfg.on_time() } else { cfg.off_time() };

        tokio::select! {
            () = tokio::time::sleep(wait) => on = !on,
            changed = config.changed() => {
                changed?;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_round_trips_through_dbus_values() {
        for action in [Action::On, Action::Off, Action::Blink] {
            assert_eq!(Action::from_dbus(action.as_str()), Some(action));
        }
        assert_eq!(
            Action::from_dbus("xyz.openbmc_project.Led.Physical.Action.Dim"),
            None
        );
        assert_eq!(Action::from_dbus(""), None);
    }

    #[test]
    fn default_config_matches_the_interface_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.action, Action::Off);
        assert_eq!(cfg.period_ms, 1000);
        assert_eq!(cfg.duty_on, 50);
    }

    #[test]
    fn blink_times_follow_the_duty_cycle() {
        let cfg = Config {
            action: Action::Blink,
            period_ms: 1000,
            duty_on: 30,
        };
        assert_eq!(cfg.on_time(), Duration::from_millis(300));
        assert_eq!(cfg.off_time(), Duration::from_millis(700));
    }

    #[test]
    fn blink_times_are_clamped_to_the_valid_range() {
        let cfg = Config {
            action: Action::Blink,
            period_ms: 1000,
            duty_on: u8::MAX,
        };
        assert_eq!(cfg.on_time(), Duration::from_millis(1000));
        assert_eq!(cfg.off_time(), Duration::from_millis(1));
    }

    #[test]
    fn blink_times_never_round_down_to_zero() {
        let cfg = Config {
            action: Action::Blink,
            period_ms: 1,
            duty_on: 1,
        };
        assert_eq!(cfg.on_time(), Duration::from_millis(1));
        assert_eq!(cfg.off_time(), Duration::from_millis(1));
    }
}
