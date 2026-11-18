//! Virtual LED controller daemon for OpenBMC.
//!
//! Requests one GPIO line as an output and publishes it on the system bus as
//! `xyz.openbmc_project.LED.Controller.VirtualTest`.

use std::future::pending;

use anyhow::Result;
use tokio::sync::watch;
use virtual_led_ctrl_rust::dbus::PhysicalLed;
use virtual_led_ctrl_rust::led::{self, Config, GpioLed};
use virtual_led_ctrl_rust::{LED_OBJECT_PATH, SERVICE_NAME};
use zbus::connection::Builder;
use zbus::fdo::ObjectManager;

const DEFAULT_CHIP: &str = "gpiochip0";
const DEFAULT_LINE: u32 = 0;

const USAGE: &str = "\
Usage: virtual-led-ctrl [OPTIONS]

Options:
      --chip <NAME>    GPIO chip to drive [default: gpiochip0]
      --line <OFFSET>  GPIO line offset within the chip [default: 0]
  -h, --help           Print this help
";

/// Command line options.
struct Options {
    chip: String,
    line: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            chip: DEFAULT_CHIP.to_owned(),
            line: DEFAULT_LINE,
        }
    }
}

impl Options {
    /// Parses the command line.
    ///
    /// Returns `Ok(None)` when help was requested.
    fn parse<I>(args: I) -> Result<Option<Self>, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut options = Self::default();
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--chip" => {
                    options.chip = args
                        .next()
                        .ok_or_else(|| "--chip requires a value".to_owned())?;
                }
                "--line" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--line requires a value".to_owned())?;
                    options.line = value
                        .parse()
                        .map_err(|_| format!("invalid GPIO line offset: {value}"))?;
                }
                _ => return Err(format!("unexpected argument: {arg}")),
            }
        }

        Ok(Some(options))
    }
}

// Like the Boost.Asio event loop of the C++ daemon, everything runs on a
// single thread.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let Some(options) = Options::parse(std::env::args().skip(1)).unwrap_or_else(|err| {
        eprintln!("{err}\n\n{USAGE}");
        std::process::exit(2);
    }) else {
        print!("{USAGE}");
        return Ok(());
    };

    let led = GpioLed::open(&options.chip, options.line)?;
    println!(
        "LED Controller initialized on {}, line {}",
        options.chip, options.line
    );

    let (config_tx, config_rx) = watch::channel(Config::default());

    // The object manager mirrors what the sdbusplus object server provided, so
    // clients can enumerate the objects exported by this service.
    let _connection = Builder::system()?
        .name(SERVICE_NAME)?
        .serve_at(LED_OBJECT_PATH, PhysicalLed::new(config_tx))?
        .serve_at("/", ObjectManager)?
        .build()
        .await?;

    tokio::spawn(led::run(led, config_rx));

    println!("Virtual LED D-Bus Service is running...");

    // The D-Bus connection and the blink task keep running in the background.
    pending::<()>().await;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        Options::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn defaults_match_the_cpp_daemon() {
        let options = parse(&[]).unwrap().unwrap();
        assert_eq!(options.chip, "gpiochip0");
        assert_eq!(options.line, 0);
    }

    #[test]
    fn chip_and_line_can_be_overridden() {
        let options = parse(&["--chip", "gpiochip1", "--line", "15"])
            .unwrap()
            .unwrap();
        assert_eq!(options.chip, "gpiochip1");
        assert_eq!(options.line, 15);
    }

    #[test]
    fn help_is_requested_with_dash_h_or_dash_dash_help() {
        assert!(parse(&["-h"]).unwrap().is_none());
        assert!(parse(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn invalid_arguments_are_reported() {
        assert!(parse(&["--line"]).is_err());
        assert!(parse(&["--line", "not-a-number"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
    }
}
