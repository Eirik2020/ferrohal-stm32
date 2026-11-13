# Driver traits, D1-D5 (API review)

Instance traits, kernel clock, exposure. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.

## Instance shapes (D1)

| driver | clock const: type | registers | RCC bit | `'static` | at |
|---|---|---|---|---|---|
| UART | `KERNEL`: `rcc::Bus` | `info() -> &'static Info` | in `Info` | y | `uart/stream_dma.rs:264` |
| SPI | `KERNEL`: `spi::Kernel` (enum) | `info()` | in `Info` | y | `spi/stream_dma.rs:90` |
| I2C | `KERNEL`: `i2c::Kernel` (= `Bus`) | `info()` | in `Info` | y | `i2c/mod.rs:88` |
| ADC | `BUS`: `rcc::Bus` | `unit() -> Unit` | `ENABLE` | n | `adc/mod.rs:90` |
| timer | `BUS`: `rcc::Bus` | `core() -> TimCore` | `ENABLE`, doc-hidden | y | `timer.rs:49` |
| SDMMC | none | `Registers::regs()` supertrait | `ENABLE` | n | `sdmmc_v2/mod.rs:135` |
| GPIO, DMA | none | `PortRegisters`/`DmaRegisters` supertrait | `ENABLE` | n | `gpio_v2/mod.rs:48`, `dma_stream.rs:64` |

## Exposure (D3)

| driver | engine | per-IP module re-exports | neutral module re-exports routing traits |
|---|---|---|---|
| UART | `uart/stream_dma.rs` | driver + traits (`usart_v2/mod.rs:12`) | no: `Line`, `Uart`, `UartIrq`, `UartRx`, `UartTx` only (`uart/mod.rs:32`) |
| SPI | `spi/stream_dma.rs` | driver + traits (`spi_v2/mod.rs:16`) | yes (`spi/mod.rs:35`) |
| I2C | `i2c/mod.rs` | errors only (`i2c_v1/mod.rs:32`) | defined there |
| ADC | `adc/mod.rs` | nothing | defined there |

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| D1 | M | 7 rows, 5+ shapes of `Instance`; nothing records which differences are deliberate | table. Vector const `INTERRUPT`/`INTERRUPT_NAME` (UART) vs `UPDATE`/`UPDATE_NAME` (timer): justified by meaning. A timer keeps no interrupt state -> nothing for an `Info` |
| D2 | M | Kernel clock: 3 types + `BUS` | `uart/stream_dma.rs:266`, `i2c/mod.rs:102`, `spi/mod.rs:125`; SDMMC reads `clocks.pll1_q`. `DEBT.md` §1: muxes forced, kernel-clock field needed before other muxes |
| D3 | M | 4 neutral drivers expose themselves 3 ways; generic board code must name the IP version | table; `hil/nucleo-h753zi-loopback/src/board.rs:762-766` bounds on `usart_v4::{Instance, TxPin, RxPin, TxDma, RxDma}` -> cannot compile for F4. UART types loop: `uart::stream_dma` -> `usart_vN` -> `uart` via private `crate::usart` |
| D4 | L | SPI `Instance`, `Info`, pin traits live in the DMA engine | `spi/stream_dma.rs:67-104`; `spi/polled.rs:19` imports them |
| D5 | M | Erasure vs token-carrying undecided | `docs/design/ownership.md`: static `Info`, type erased. `docs/design/free.md` rule 1: every handle carries its token. `docs/plans/2026-11-08-free-redesign.md:14`: `Info`-only drivers = blocker ("store ZST tokens"). `Tick<T>`/`Timebase<T>` went generic (#35) |

## Fix (opinion)

- D1: one trait shape: `KERNEL: Kernel`, `NAME`, `info() -> &'static Info` (`RccBit` inside), `'static`. Record the deliberate exceptions.
- D2: SPI's enum -> `rcc::Kernel`, used by UART, SPI, I2C. Mechanical while small.
- D3, D4: neutral module defines + re-exports all a board names (driver, `Instance`, pin + DMA traits, `Wiring`); per-IP module = `Unit` + register ops, re-exports nothing. Removes the loop.
- D5: trait shape can converge now; driver genericity waits on the `free()` redesign.
