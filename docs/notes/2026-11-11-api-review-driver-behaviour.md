# Errors, waits and vectors, D6-D13 (API review)

Error types, refusals, waits, vectors. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| D6 | M | `uart::ConfigError` = the crate's DMA-buffer error | `chip::check_dma_buffer` is public, returns it: `chips/stm32f401re.rs:38`, `stm32f405rg.rs:44`, `shared/h7.rs:45`. Lane + sampler refusals: `timer.rs:172`, `:176`, `timer_pwm.rs:65`, `:70`, `:77`, `:435`. SPI `From` + wildcard arm: `spi/mod.rs:272`. ADC flattens to `Error::Buffer`. PWM user matches `uart::ConfigError::DmaBusy` |
| D7 | M | Bad sampler rate -> `uart::ConfigError::Unsupported`; timer `ConfigError` dropped | `timer_pwm.rs:859`; bench asserts it: `hil/nucleo-f401re-loopback/src/bin/run_v.rs:499` |
| D8 | L | Refusals carry 3 error families | `ConfigError`: UART, SPI, I2C, timers, PWM, AWG. `Error`: ADC (`adc/mod.rs:214`), IWDG (`iwdg.rs:147`), SDMMC (`sdmmc_v2/mod.rs:228`). `ClockError`: RCC, USB (`usb.rs:36`). ADC `Error::KernelClock` sits beside runtime `Overrun`; I2C, SPI keep `KernelClock` in `ConfigError` |
| D9 | L | Neutral `usb.rs` hard-codes PA11/PA12 AF10: chip fact outside the glue | `usb.rs:40-41`, `:52-53` |
| D10 | L | No `RESOURCES` on USB, SDMMC wiring; the other 5 have it | `sdmmc_v2/mod.rs:155` |
| D11 | M | 2 wait mechanisms | `wait::Deadline` (UART, SPI, I2C; honours `CycleClock`). 17 raw spin loops in 9 files: RCC x2, ADC x2, SDMMC, IWDG, `i2c_v1`, `spi_v2`, DMA |
| D12 | L | Interrupt service not uniform | `on_interrupt` -> `Option<Result<..>>` where a completion exists: `spi/stream_dma.rs:379`, `timer_pwm.rs:759`, `:957`, `timer_awg.rs:446`, `adc/mod.rs:400`. No `poll`: `UartIrq`, `Tick` (`UartTx::poll` services the TX queue). `Exti`: neither |
| D13 | L | Vectors to bind: 4 shapes; `verify()` on 5 of 9 | struct + `verify()`: `UartInterrupts`, `SpiInterrupts`. `interrupt()` + `verify()`: `Tick`, `CompareLane`, `Sampler`. `interrupt()` only: `AdcScan`, `Exti`, `Awg`. Assoc fn: `UsbFs::interrupt()` |

## Fix (opinion)

- D6: `dma::BufferError { DmaBusy, BufferUnreachable, BufferCacheable }` (or in `resource`); each driver's `ConfigError` wraps it.
- D7: sampler refuses with the timer's `ConfigError`; falls out of D6. Update run V + H753 run U.
- D8: rule "a refusal carries `ConfigError`"; fold in ADC, IWDG, SDMMC.
- D9: `DmPin`/`DpPin` routing traits from the glue, as every other driver.
- D10: `RESOURCES` on both, via U5's helper.
- D11: `Deadline` everywhere a CPU-cycle bound makes sense; constants per U8.
- D12, D13: one `Interrupts` type with `iter()` + `verify()` per driver; `poll` where `on_interrupt` reports a completion.
