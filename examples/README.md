# FerroHAL examples

Small programs, one per driver, each on a board you can buy: start here to see how FerroHAL is used. Each is one file, top to bottom: clocks, pins, the driver, a loop. The bench crates under `hil/` prove the drivers; these only show them.

FerroHAL owns no scheduler. These examples use plain `cortex-m-rt`, an `#[entry]` and a loop; a firmware under RTIC or any other scheduler opens the same drivers the same way and calls their service methods from its own handlers.

## Running one

You need the `thumbv7em-none-eabihf` target (`rustup target add thumbv7em-none-eabihf`) and [probe-rs](https://probe.rs). Plug the board's ST-LINK in, then from the board's directory:

```sh
cd examples/nucleo-f401re
cargo run --release --bin blinky
```

`cargo run` builds, flashes over the ST-LINK and prints the program's `defmt` log. Each crate is its own Cargo workspace, so its `.cargo/config.toml` picks the target, the linker scripts and the probe-rs chip.

## The examples

| Board | Binary | Driver | Wiring |
|---|---|---|---|
| Nucleo-F401RE | `blinky` | GPIO: LD2 blinks, faster while B1 is held | none |
| Nucleo-F401RE | `pwm-fade` | Timer PWM: LD2 fades on TIM2 channel 1 | none |
| Nucleo-F401RE | `adc-read` | ADC: A0 in millivolts, the supply and die temperature | a voltage of 0 to 3.3 V on A0 |
| Nucleo-F401RE | `spi-loopback` | Polled SPI master: SPI2 reads back what it sends | CN10-26 (PB15, MOSI) to CN10-28 (PB14, MISO) |
| Nucleo-F401RE | `i2c-scan` | I2C master: lists the addresses that answer on I2C1 | an I2C device on D15 (SCL) and D14 (SDA) |
| Nucleo-H753ZI | `usb-serial` | USB device: a CDC-ACM serial port that echoes | a USB cable to CN13 |

Each file's header names its pins, its connectors and what to expect.

## What they show

- **Ownership.** `Peripherals::take()` hands out each peripheral once; `Rcc` and each port's `ctrl` own the registers drivers share.
- **Routing checked at compile time.** A pin or alternate function a peripheral does not have, or a timer channel a pin does not carry, does not build.
- **Refusals hand the parts back.** Every constructor checks its configuration first and, when it refuses, returns the tokens it was given with the reason.
- **No hidden waits.** Every wait is bounded, and an error is a value to handle: FerroHAL's own drivers have no reachable panic (the `usb` feature's third-party stack is the listed exception). The examples panic only where the program cannot go on.

## Kept building

`checks/run.sh` builds every example with `--locked` and checks its formatting, so CI fails on a rename that would break one.
