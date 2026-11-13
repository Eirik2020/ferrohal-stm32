# Naming rules, N1-N5 (API review)

ADR-003 rule 5 against the code. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`. ADR-003
rule 5 fixes a vocabulary (channel, lane, stream, line, handle; `set_*`/`into_*`;
`*_us`/`*_hz`). Below: where code breaks it, and families it does not mention. N6 on: [2026-11-11-api-review-naming-drift.md](2026-11-11-api-review-naming-drift.md).

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| N1 | M | Refusal aliases: 4 patterns + a `Result` alias | `<Type>Refused`: `ScanRefused` (`adc/mod.rs:39`), `AwgRefused` (`timer_awg.rs:35`). `<Type>NewRefused`: `SdmmcNewRefused` (`sdmmc_v2/mod.rs:228`), `UsbNewRefused` (`usb.rs:36`), `SamplerNewRefused` (`timer_pwm.rs:76`). `<method>Refused`: `IntoLaneRefused`, `IntoTwoWayLaneRefused` (`timer_pwm.rs:64`, `:69`). `<Type>Refusal`: `LaneRefusal`, `ComplementaryLaneRefusal` (`timer.rs:172`, `:176`). `TwoWayLaneAssembly` (`timer.rs:183`). UART, SPI, I2C: none, `#[allow(clippy::type_complexity)]` |
| N2 | L | Module-prefix stutter mixed; `clippy::module_name_repetitions`: 17 items | `uart`: `UartInterrupts`, `UartIrq`, `UartRx`, `UartTx` beside `Health`, `TxHealth`, `Buffers`, `Line`, `Chunk`. `spi`: `SpiInterrupts`, `SpiTxDma`, `SpiRxDma` beside `Buffers`, `Health`; UART has plain `TxDma`/`RxDma`. `adc`: `AdcPin`, `AdcDma`, `AdcWiring`, `AdcScan` beside `Channel`, `Config`, `Error`; others plain `Wiring`. `AdcPin` defensible: glue imports `gpio::Pin` |
| N3 | M | One quantity, 4 names | `i2c::Config::new(speed)` (`i2c/mod.rs:153`), read back by `I2c::frequency()`; `spi`, `sdmmc` `new(frequency)` (`spi/mod.rs:78`, `sdmmc_v2/mod.rs:122`); errors `Speed` vs `Frequency`. `Pwm::new(.., rate)` (`timer_pwm.rs:145`) but `set_frequency(rate)` (`:176`); `Sampler::set_rate` (`:900`). `uart::Config.baudrate: u32`, not `Hertz`. "Speed" = GPIO slew too (`gpio_v2/mod.rs:128`, `:223`) |
| N4 | M | `dma::Channel` is an erased stream; 3 meanings of "channel" in one module | `dma_stream.rs:216-217`; made by `claim_channel`; F4 "channel" = CHSEL request (`dma_stream.rs:6`); rule 5: stream = DMA, channel = timer compare. `adc/mod.rs:30` imports it `as DmaChannel`. ~30 references, 0 under `hil/` |
| N5 | L | "Handle" has no fixed shape | `ExtiHandle` (`exti.rs:217`), `FlagHandle` (`dma_stream.rs:581`), `TimebaseReader` (`timer.rs:416`), `Watchdog::handle() -> Self` (`iwdg.rs:248`) = `Clone` renamed. `UartIrq` plays rule 5's handle role, named `Irq` |

## Fix (opinion)

- N1: `<Type><Method>Refused`, or name by what fails (`LaneRefused`); kill `Refusal` first.
- N2: no module prefix, except the main driver type and glue-file collisions; add to rule 5.
- N3: `frequency` = clock, `rate` = events/s (tick, PWM, sampler), `baudrate` stays; "speed" = GPIO only.
- N4: `AnyStream` or `ErasedStream`; `claim_channel` -> `claim`.
- N5: rule 5 defines "handle" and these follow, or drops the word.
