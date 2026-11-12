# One concept, several names, N6-N19 (API review)

One concept, several names; small items. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.
N1-N5: [2026-11-11-api-review-naming.md](2026-11-11-api-review-naming.md).

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| N6 | L | Small | Used `_` bindings (`_adc`, `_timer`, `_wiring`, `_token`, `_dm`, `_dp`): `clippy::used_underscore_binding` 13 on H753, 11 on F405 (`adc/mod.rs:216`, `timer_pwm.rs:149`, `sdmmc_v2/mod.rs:256`, `usb.rs:56`). Private aliases `adc_ip`, `i2c_ip`, `spi_ip` vs `usart` (`lib.rs:126-164`). `vdda_mv` (`adc/mod.rs:247`) beside `vdda_millivolts()`, `to_millivolts()`. `pub type Block = Regs` (`i2c_v1/mod.rs:40`, `i2c_v2/mod.rs:26`): unused; SPI, UART say `Regs` |
| N7 | L | Register wrapper | `Unit` (SPI, I2C, ADC); `LineRegs` (UART). ADC `Unit`: `pub const fn new`, methods take `&self`; the others `pub(crate)`, `self` by value |
| N8 | L | Routing traits + wiring beyond N2 | `CompareDma`/`UpdateDma`, `LaneWiring` (timer); USB: positional token + pins, no wiring |
| N9 | L | Vector name const | `INTERRUPT`/`INTERRUPT_NAME` (UART); `UPDATE`/`UPDATE_NAME` (timer); `INTERRUPT`/`NAME` (`StreamInterrupt`) |
| N10 | L | DMA fault variant | `DmaTransfer` + `DmaOverrun` (UART); `Dma` (SPI, ADC, timer) |
| N11 | L | Buffer refusal | `BufferUnreachable` + `BufferCacheable` (UART, SPI); `Buffer` (ADC) |
| N12 | L | Health counters | singular fields, atomics, wrap (UART); plural, `saturating_add` x9 (SPI); plural, plain `+= 1` x8 (I2C); none (ADC, timer) |
| N13 | L | RCC error | F4 `Usb`, `Sysclk`, `Pclk1`, `Pclk2`; H7 `UsbTimeout`, `Cpu`, `Pclk` |
| N14 | L | Give back | `free` (`Tick`, `Timebase`); `release` (`Exti`) |
| N15 | L | Stop a transfer | `abort` (`Spi`, `AdcScan`, `CompareLane`, `Awg`); `stop` (`Sampler`) |
| N16 | L | Mask events | `Line::mask`/`unmask` (UART); `Exti::listen`/`unlisten`; `listen` = capture on `CompareLane`, `Awg` |
| N17 | L | Constructor | `new`; `Watchdog::start` (`iwdg.rs:144`); token `chip::WatchdogPeripheral`, others carry the peripheral's name |
| N18 | L | Accessors | `name()`: `I2c`, `BlockingSpi`, not `Spi`. `kernel_clock()`: `I2c` only. `health()`: not `BlockingSpi`. `check::<U>()`: `Uart`, `Spi`, not `I2c` |
| N19 | L | `Config` | `with_*` builders: UART, SPI only. `defmt::Format` hand-written: `spi::Config`, `i2c::Config` (`Hertz` derives it). `adc::Config`: `slow_source()`, no `new` |

## Fix (opinion)

- N6: `usart_ip`; `*_mv` or the long form into rule 5; drop `Block`; no `_` on used bindings.
- N7: `Unit` everywhere, `pub(crate)`, by value.
- N8: fold into N2's rule; USB gets a `Wiring` (with D9).
- N9: `INTERRUPT`/`INTERRUPT_NAME`; `UPDATE` stays where the meaning differs.
- N10, N11: fall out of D6's shared error.
- N12: plural fields; `saturating_add` (or atomics) in every driver.
- N13: one set, in the shared RCC module (U2).
- N14: `free`. N15: `abort`. N16: `mask`/`unmask` for interrupt enables; `listen` = capture only.
- N17: `Watchdog::new` or record `start` as deliberate; token `IWDG`.
- N18, N19: same accessor and builder set on every driver.
