//! Behavioural tests for the blink scheduler, driven with a paused clock and an
//! in-memory LED output.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use virtual_led_ctrl_rust::led::{self, Action, Config, LedOutput};

/// LED output that records every state written to it.
#[derive(Clone, Default)]
struct RecordingLed {
    states: Arc<Mutex<Vec<bool>>>,
}

impl RecordingLed {
    fn states(&self) -> Vec<bool> {
        self.states.lock().unwrap().clone()
    }
}

impl LedOutput for RecordingLed {
    fn set(&self, on: bool) {
        self.states.lock().unwrap().push(on);
    }
}

fn blink(period_ms: u16, duty_on: u8) -> Config {
    Config {
        action: Action::Blink,
        period_ms,
        duty_on,
    }
}

/// Starts the LED task and returns the recording output, the configuration
/// sender, and the task handle.
fn start(config: Config) -> (RecordingLed, watch::Sender<Config>, JoinHandle<()>) {
    let led = RecordingLed::default();
    let (tx, rx) = watch::channel(config);
    let handle = tokio::spawn(led::run(led.clone(), rx));
    (led, tx, handle)
}

/// Advances the paused clock by `ms` and lets the LED task react.
async fn advance(ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    settle().await;
}

/// Gives the LED task a chance to run until it is idle again.
async fn settle() {
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test(start_paused = true)]
async fn off_drives_the_line_low() {
    let (led, _tx, handle) = start(Config::default());
    settle().await;

    assert_eq!(led.states(), [false]);
    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn on_drives_the_line_high() {
    let (led, _tx, handle) = start(Config {
        action: Action::On,
        ..Config::default()
    });
    settle().await;

    assert_eq!(led.states(), [true]);
    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn blink_toggles_once_per_half_period_at_fifty_percent_duty() {
    let (led, _tx, handle) = start(blink(1000, 50));
    settle().await;

    // Blinking always starts with the LED on.
    assert_eq!(led.states(), [true]);

    advance(499).await;
    assert_eq!(led.states(), [true]);

    advance(1).await;
    assert_eq!(led.states(), [true, false]);

    advance(500).await;
    assert_eq!(led.states(), [true, false, true]);

    advance(500).await;
    assert_eq!(led.states(), [true, false, true, false]);

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn blink_respects_an_asymmetric_duty_cycle() {
    let (led, _tx, handle) = start(blink(1000, 25));
    settle().await;
    assert_eq!(led.states(), [true]);

    // On for 250ms, then off for 750ms.
    advance(250).await;
    assert_eq!(led.states(), [true, false]);

    advance(749).await;
    assert_eq!(led.states(), [true, false]);

    advance(1).await;
    assert_eq!(led.states(), [true, false, true]);

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn full_duty_blink_is_solid_on() {
    let (led, _tx, handle) = start(blink(1000, 100));
    settle().await;
    assert_eq!(led.states(), [true]);

    advance(5000).await;
    assert_eq!(led.states(), [true]);

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn zero_duty_blink_is_solid_off() {
    let (led, _tx, handle) = start(blink(1000, 0));
    settle().await;
    assert_eq!(led.states(), [false]);

    advance(5000).await;
    assert_eq!(led.states(), [false]);

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn changing_the_state_stops_blinking() {
    let (led, tx, handle) = start(blink(1000, 50));
    settle().await;
    advance(500).await;
    assert_eq!(led.states(), [true, false]);

    tx.send_modify(|config| config.action = Action::Off);
    settle().await;
    assert_eq!(led.states().last(), Some(&false));

    // No further toggling once the LED is off.
    let len = led.states().len();
    advance(5000).await;
    assert_eq!(led.states().len(), len);

    handle.abort();
}

#[tokio::test(start_paused = true)]
async fn changing_the_period_restarts_the_blink_cycle() {
    let (led, tx, handle) = start(blink(1000, 50));
    settle().await;
    advance(500).await;
    assert_eq!(led.states(), [true, false]);

    tx.send_modify(|config| config.period_ms = 200);
    settle().await;
    // The cycle restarts from the on phase with the new period.
    assert_eq!(led.states(), [true, false, true]);

    advance(100).await;
    assert_eq!(led.states(), [true, false, true, false]);

    handle.abort();
}
