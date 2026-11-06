# ferrohal-stm32

A scheduler-agnostic STM32 HAL on [`stm32-metapac`](https://crates.io/crates/stm32-metapac): RTIC-first, never RTIC-only. It's being built as the hardware layer for the FerroWasp flight controller firmware, and is a prototype: not on crates.io, and the API still moves.

## What it is

- **No scheduler inside.** FerroHAL never defines an interrupt handler, writes the NVIC, or takes a critical section. Drivers enable their peripheral's own interrupt bits and expose `on_interrupt()`-style service methods for the scheduler's handlers to call. Every wait is bounded: counted, or measured with a `CycleClock` the firmware provides.
- **No reachable panics.** `checks/no-panic` links the whole public API under a panic handler that calls an undefined symbol, so a release build only links if no panic path survives.
- **One module per IP version.** Drivers are written per metapac IP version (`usart_v2`, `usart_v4`, `spi_v2`, `spi_v4`, `dma_v1`/`dma_v2`, `exti_v1`/`exti_h7`, `gpio_v2`, `rcc_f4`, `rcc_h7rm0433`). A chip is a glue file of facts (pins, alternate functions, DMA routes, RCC bits, memory map) that `tools/check_glue.py` checks against metapac's metadata.
- **Routing checked at compile time.** A board file names the pins and DMA streams; a pin without the peripheral's function, or a stream that can't serve its request, doesn't build. Wirings also describe themselves as `const` data (`Wiring::RESOURCES`) for comparison with a board manifest.

## Chips and peripherals

| Chip | Board it's tested on |
|---|---|
| STM32F401RE | Nucleo-F401RE |
| STM32F405RG | Foxeer F405 V2 |
| STM32H753ZI | Nucleo-H753ZI |
| STM32H743VI | TBS Lucid H7 |

- **RCC:** explicit clock trees from HSI or HSE, validated by a `const fn`.
- **GPIO:** typestate pins, one owner per port's shared registers.
- **DMA:** the stream DMA as it is, with DMAMUX on the H7.
- **UART over DMA:** a never-stopping receive ring with idle- and error-ended chunks, queued transmit frames, receive-only UARTs, RX/TX inversion (H7), `embedded-io`.
- **SPI master over DMA:** one full-duplex transfer at a time, finished by the receive stream's interrupt or by polling, plus a bounded `embedded-hal` `SpiBus`.
- **SPI without DMA:** a polled, bounded `SpiBus`, for a bus whose streams are taken.
- **I2C master:** a bounded `embedded-hal` bus for the device drivers (the barometer, the GNSS's DDC mode), over both I2C generations: the F4's SR1/SR2 register set and the H7's TIMINGR one, with the timing computed from the kernel clock, a NACK reported with the bus put back, and the wiring handed back on a refusal.
- **EXTI:** GPIO lines with typed line tokens; a line can watch a pin a peripheral drives.
- **Timers:** an exact-rate tick, a 32-bit timebase, PWM on CHx and CHxN, compare registers fed by DMA (the DShot transmit primitive), timer-paced sampling of a GPIO port, and lane wirings that describe themselves and assemble with their tokens handed back on a refusal.
- **Watchdog:** the independent watchdog, and the reason for the last reset.
- **USB:** full-speed device for `usb-device` and `usbd-serial`, behind the `usb` feature.
- **ADC:** one channel at a time or a sequence over DMA, with the internal reference and temperature sensor against their factory calibration.
- **System:** restarting into the ROM bootloader, and the unique device ID.

Not yet: SDMMC, BDMA, input capture.

## Checking it

```sh
checks/run.sh        # clippy, no reachable panic, glue against metapac, host tests: all four chips
hil/run-f401.sh      # the Nucleo-F401RE bench
hil/run-h753.sh      # the Nucleo-H753ZI bench
hil/run-foxeer.sh    # the Foxeer F405 V2 bench, over its USB port
hil/run-lucid.sh     # the TBS Lucid H7 bench, over its USB port
```

The benches flash RTIC firmwares over probe-rs and need the jumpers listed at the top of each board's `hil/*/src/board.rs` (UART TX-to-RX links, SPI MOSI-to-MISO links). The flight controllers have no probe: their firmware is flashed through the ROM DFU bootloader and reports over USB, against each board's own crystal, IMU, battery inputs and, on the Foxeer, SPI NOR.

## Status

[DEBT.md](DEBT.md) lists what's open, per chip; [RESOLVED.md](RESOLVED.md) is the history of what's been closed.

## Licence

MIT OR Apache-2.0, at your option. Parts are ported from embassy-stm32; see [THIRD-PARTY.md](THIRD-PARTY.md).
