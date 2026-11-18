# virtual-led-ctrl-rust

Rust re-implementation of the `virtual-led-ctrl` C++ daemon. It drives a single
GPIO line and exports it on the system bus through the
`xyz.openbmc_project.Led.Physical` interface, so `phosphor-led-manager`,
Redfish and `busctl` clients can switch a physical LED on, off, or blink it.

| Item | Value |
| :--- | :--- |
| D-Bus service name | `xyz.openbmc_project.LED.Controller.VirtualTest` |
| Object path | `/xyz/openbmc_project/led/physical/virtual_test` |
| Interface | `xyz.openbmc_project.Led.Physical` |
| Default GPIO | `gpiochip0` line `0` (override with `--chip` / `--line`) |

## Requirements

* Rust 1.84 or newer (`edition 2021`, MSRV aligned with the OpenBMC Yocto
  `rust_1.84.1` recipe).
* Linux with the GPIO character device (`/dev/gpiochipN`, uAPI v2, kernel 5.10+).
* A running system bus.

The only runtime libraries needed are `libc` and `libgcc_s`: the D-Bus stack
([zbus](https://crates.io/crates/zbus)) and the GPIO stack
([gpiocdev](https://crates.io/crates/gpiocdev)) are pure Rust, so neither
sdbusplus, Boost nor libgpiod is required.

## Build and test

```sh
cargo build --release
cargo test
cargo run -- --chip gpiochip0 --line 0
```

## Command line

```
Usage: virtual-led-ctrl [OPTIONS]

Options:
      --chip <NAME>    GPIO chip to drive [default: gpiochip0]
      --line <OFFSET>  GPIO line offset within the chip [default: 0]
  -h, --help           Print this help
```

The binary is named `virtual-led-ctrl`, exactly like the C++ daemon, so
existing systemd units and Yocto recipes keep working unchanged. Boards where
the LED is not on line 0 (for example GPIOB7 on the AST2500) can simply pass
`--line 15` instead of carrying a source patch.

## D-Bus interface

`xyz.openbmc_project.Led.Physical`:

| Property | Signature | Default | Description |
| :--- | :--- | :--- | :--- |
| `State` | `s` | `...Action.Off` | `Action.On`, `Action.Off` or `Action.Blink` |
| `Period` | `q` | `1000` | Blink period in ms for one on/off cycle |
| `DutyOn` | `y` | `50` | Percentage of the period the LED is on |

Introspection, including the `org.freedesktop.DBus.ObjectManager` provided at
`/`:

```sh
busctl introspect xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test

busctl call xyz.openbmc_project.LED.Controller.VirtualTest \
    / org.freedesktop.DBus.ObjectManager GetManagedObjects
```

Drive the LED:

```sh
# Solid on
busctl set-property xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test \
    xyz.openbmc_project.Led.Physical State s \
    xyz.openbmc_project.Led.Physical.Action.On

# Blink at 2 Hz, 25% duty cycle
busctl set-property xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test \
    xyz.openbmc_project.Led.Physical State s \
    xyz.openbmc_project.Led.Physical.Action.Blink
busctl set-property xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test \
    xyz.openbmc_project.Led.Physical Period q 500
busctl set-property xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test \
    xyz.openbmc_project.Led.Physical DutyOn y 25

# Read back
busctl get-property xyz.openbmc_project.LED.Controller.VirtualTest \
    /xyz/openbmc_project/led/physical/virtual_test \
    xyz.openbmc_project.Led.Physical State
```

`DutyOn = 100` blinks as solid on and `DutyOn = 0` as solid off, matching the
C++ behaviour. Every accepted property write emits `PropertiesChanged`, and a
write while blinking restarts the blink cycle from the on phase.

## Cross-compiling for OpenBMC

OpenBMC's Yocto SDK ships no Rust toolchain, so build with a host `rustup`
toolchain for the target that matches the SDK (`arm1176jzs` is ARMv6 soft
float):

```sh
rustup target add arm-unknown-linux-gnueabi
source /usr/local/oecore-x86_64/environment-setup-arm1176jzs-openbmc-linux-gnueabi

export CARGO_TARGET_ARM_UNKNOWN_LINUX_GNUEABI_LINKER=arm-openbmc-linux-gnueabi-gcc
export RUSTFLAGS="-C target-cpu=arm1176jzf-s \
    -C link-arg=--sysroot=/usr/local/oecore-x86_64/sysroots/arm1176jzs-openbmc-linux-gnueabi"

cargo build --release --target arm-unknown-linux-gnueabi
```

The resulting `target/arm-unknown-linux-gnueabi/release/virtual-led-ctrl` is a
static Rust binary that only needs `libc.so.6` and `libgcc_s.so.1` from the
image. Copy it to `/usr/bin/virtual-led-ctrl` on the BMC and keep the existing
`virtual-led-ctrl.service` from `meta-custom`.

## Differences from the C++ implementation

| Area | C++ (`virtual-led-ctrl`) | Rust |
| :--- | :--- | :--- |
| GPIO | libgpiod v1 (`libgpiodcxx`) | `gpiocdev`, raw character device uAPI v2 |
| D-Bus | sdbusplus + Boost.Asio | zbus + Tokio, single-threaded runtime |
| Event loop | `io_context::run()` | Tokio `current_thread` runtime |
| GPIO line | Hard-coded `gpiochip0`/`0` + layer patch | `--chip` / `--line` options |
| `PropertiesChanged` | Not emitted | Emitted by zbus on every accepted write |
| Input validation | None | Unknown `State`, `Period = 0` and `DutyOn > 100` are rejected with `InvalidArgs` |

## Layout

```
src/lib.rs    crate root: service name and object path
src/led.rs    GPIO output, configuration and blink scheduler (+ unit tests)
src/dbus.rs   xyz.openbmc_project.Led.Physical interface
src/main.rs   daemon entry point and command line parsing
tests/blink.rs blink behaviour, driven by a paused clock and a mock output
```
