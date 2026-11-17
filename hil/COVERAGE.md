# HIL test coverage

Which bench proves what, per IP version: the unit the car rule proves on one chip for
every chip that carries it (`docs/design/requirements-system.md`). Runs are per bench
(run letters differ between benches, review B1); each run's procedure is in
[`requirements/benches.md`](../requirements/benches.md). What the chips support is
[`docs/CHIPS.md`](../docs/CHIPS.md); the rig itself and its upgrade queue are
[CAPABILITIES.md](CAPABILITIES.md); open proof gaps are `docs/DEBT.md` §3 and §4.

Tiers ([`docs/design/maturity.md`](../docs/design/maturity.md)), as `reqtool.toml` gives
each bench and every record is stamped: the Nucleos and the STM32F4 Discovery are `HIL`;
the Foxeer F405 V2 and the TBS Lucid H7 are `FLSH`, flashed and run without full debugger
coverage (ADR-009). Measured 2026-11-15: F401 21 runs and the glue probe, 421 checks; H753
23 runs and the probe, 718; Discovery 3 runs and the probe, 143; reqtool 119 of 122
verified.

## Computed tiers

Each IP version, die and series, with the tier its evidence holds and the benches it
holds on, computed by `glue-gen tiers` from reqtool's evidence (ADR-009); the merge gate
regenerates it. The tables after it are hand-written: which runs prove what, and the gaps.

<!-- generated: glue-gen coverage -->
| axis | tier | benches |
|---|---|---|
| IP `adc_v2` | HIL | nucleo-f401re-loopback |
| IP `adc_v4` | HIL | nucleo-h753zi-loopback |
| IP `dma_v1` | – |  |
| IP `dma_v2` | – |  |
| IP `exti_h7` | HIL | nucleo-h753zi-loopback |
| IP `exti_v1` | HIL | nucleo-f401re-loopback |
| IP `i2c_v1` | – | nucleo-f401re-loopback |
| IP `i2c_v2` | – |  |
| IP `iwdg_v1` | HIL | nucleo-f401re-loopback |
| IP `iwdg_v2` | HIL | nucleo-h753zi-loopback |
| IP `otg_v1` | HIL | nucleo-h753zi-loopback, stm32f4-discovery |
| IP `sdmmc_v2` | FLSH | lucid-h7 |
| IP `spi_v2_i2s` | – | nucleo-f401re-loopback |
| IP `spi_v3_i2s` | – |  |
| IP `spi_v4_i2s` | – | nucleo-h753zi-loopback |
| IP `timer_v1` | HIL | nucleo-f401re-loopback, stm32f4-discovery |
| IP `timer_v3` | HIL | nucleo-h753zi-loopback |
| IP `usart_v2` | HIL | nucleo-f401re-loopback |
| IP `usart_v4` | HIL | nucleo-h753zi-loopback |
| die `0x413` (glue probe) | – |  |
| die `0x419` (glue probe) | – |  |
| die `0x421` (glue probe) | – |  |
| die `0x431` (glue probe) | – |  |
| die `0x433` (glue probe) | – |  |
| die `0x441` (glue probe) | – |  |
| die `0x449` (glue probe) | – |  |
| die `0x450` (glue probe) | – |  |
| die `0x451` (glue probe) | – |  |
| die `0x452` (glue probe) | – |  |
| series F4 platform | – | nucleo-f401re-loopback, stm32f4-discovery |
| series F7 platform | – |  |
| series H7 platform | – | nucleo-h753zi-loopback |
<!-- end generated -->

## Per IP version

| IP version | chips | proven on (`HIL`): runs | flashed (`FLSH`) | not proven (`docs/DEBT.md`) |
|---|---|---|---|---|
| `usart_v2` | F401, F405 | F401: A, B, C, D, V, W | – | USART2's break, baud and parity (ST-LINK); parity clear above 115200 |
| `usart_v4` | H743, H753 | H753: A, B, C, D, E, U, W, Y | – | UART8 TX and the morpho-only pins |
| `spi_v2` | F401, F405 | F401: E, F, G (DMA), K (polled); E samples the wire (bit order, phase) | Foxeer: IMU, NOR | – |
| `spi_v4` | H743, H753 | H753: F, G, H (DMA), L (polled); F samples the wire (bit order, phase); H again with caches on | Lucid: IMU | – |
| `i2c_v1` | F401, F405 | F401: Q, S (Pico simulator) | – | – |
| `i2c_v2` | H743, H753 | H753: R, S (Pico simulator) | Lucid: barometer | – |
| `timer_v1` | F401, F405, F407 | F401: H, I, N, O, P, R; Discovery: D (CHxN) | Foxeer: motor timers, outputs off | – |
| `timer_v3` | H743, H753 | H753: I, J, P, Q (CHxN on TIM1) | Lucid: motor timers | – |
| `iwdg_v1` | F401, F405 | F401: J | Foxeer: started and fed | – |
| `iwdg_v2` | H743, H753 | H753: K (window, freeze, LSI) | – | – |
| `exti_v1` | F401, F405 | F401: E, F, G | Foxeer: IMU data ready | – |
| `exti_h7` | H743, H753 | H753: F, G, H | Lucid: IMU data ready | – |
| `dma_v2` | F401, F405 | F401: UART and SPI runs, all 9 USART and 10 SPI routes; I, O, P | – | – (double buffering and FIFO thresholds removed) |
| `dma_v1` + DMAMUX1 | H743, H753 | H753: all 16 DMA1/DMA2 streams; J, Q | – | – (double buffering and FIFO thresholds removed) |
| `gpio_v2` | all four | every Nucleo run | – | – |
| `adc_v2` | F401, F405 | F401: M | Foxeer: battery inputs | – |
| `adc_v4` | H743, H753 | H753: O | Lucid: battery inputs | – |
| `rcc_f4` | F401, F405 | F401: every run, HSE bypass | Foxeer: HSE crystal to 168 MHz | HSE crystal |
| `rcc_h7_rm0433` | H743, H753 | H753: every run, HSE bypass; I (voltage scale) | Lucid: HSE crystal to 400 MHz | HSE crystal |
| `sdmmc_v2` | H743, H753 | – | Lucid: a card (SDMMC-004), the empty slot (SDMMC-003) | error paths |
| USB, F4 OTG_FS core | F401, F405, F407 | Discovery: B (CDC echo on CN5) | Foxeer: CDC echo | – |
| USB, H7 OTG core at FS | H743, H753 | H753: M (CN13) | Lucid: CDC echo | – |

## Shared engines and system

| what | proven on: runs | not proven |
|---|---|---|
| UART engine (`uart/stream_dma.rs`), both IP versions | the UART runs on both Nucleos | UARTDMA-009 (a stream that will not stop) |
| SPI engines, DMA and polled | the SPI runs on both Nucleos | – |
| Waveform generator, two-way lanes | F401: O, P, R; H753: Q | – |
| Reset cause, ROM bootloader request | F401: L; Discovery: C (ROM DFU on CN5); H753: N, Z (ROM DFU on CN13) | RESET-004 on both families |
| UID and die revision | every run's closing log line | – |
| Caches and MPU shapes (H7) | H753: X; A to E with caches on | the SPI runs with caches on |
| Constructor refusals | F401: V; H753: U; both smoke benches | – |
| Injected faults | F401: W; H753: W, T (streams that will not stop, `fault-injection`) | – |
| No reachable panic, glue against metapac, host tests | `checks/run.sh`, no board | the `usb` dependency (a listed G2 exception) |

## Chip glue

One chip proves a family's shared glue. Every part's rows are generated from stm32-metapac
(`tools/glue-gen`, regenerated at the merge gate), and the glue probe checks each die's
RCC bits, peripheral addresses and vectors on the silicon: 87 checks on die 0x433, 116 on
0x413, 154 on 0x450.

| family | proven on | rows on glue check only |
|---|---|---|
| F4 | F401: every pin entry and DMA route of its three USARTs and three SPIs | the F405's USART3, UART4/5, TIM6-8, TIM12-14 |
| H7 | H753: 7 of 8 U(S)ARTs, 24 of 46 TX/RX entries (Zio only), SPI1-5 on Zio pins, all 16 streams | UART8 TX, the morpho-only pins, the H743's TFBGA100 rows (pinned by a test) |
