# HIL test coverage

Which bench proves what, per IP version: the unit the car rule proves on one chip for
every chip that carries it (`docs/design/requirements-system.md`). Runs are per bench
(run letters differ between benches, review B1); each run's procedure is in
[`requirements/benches.md`](../requirements/benches.md). What the chips support is
[`docs/CHIPS.md`](../docs/CHIPS.md); the rig itself and its upgrade queue are
[CAPABILITIES.md](CAPABILITIES.md); open proof gaps are `docs/DEBT.md` §3 and §4.

The Foxeer F405 V2 and the TBS Lucid H7 are **smoke tests**: they show a chip runs, and
carry no evidence. Measured 2026-11-13: F401 21 runs, 331 checks; H753 23 runs, 560
checks; reqtool 117 of 122 verified.

## Per IP version

| IP version | chips | proven on: runs | smoke test | not proven (`docs/DEBT.md`) |
|---|---|---|---|---|
| `usart_v2` | F401, F405 | F401: A, B, C, D, V, W | – | USART2's break, baud and parity (ST-LINK); parity clear above 115200 |
| `usart_v4` | H743, H753 | H753: A, B, C, D, E, U, W, Y | – | UART8 TX and the morpho-only pins |
| `spi_v2` | F401, F405 | F401: E, F, G (DMA), K (polled) | Foxeer: IMU, NOR | bit order |
| `spi_v4` | H743, H753 | H753: F, G, H (DMA), L (polled) | Lucid: IMU | bit order; caches on |
| `i2c_v1` | F401, F405 | F401: Q, S (Pico simulator) | – | – |
| `i2c_v2` | H743, H753 | H753: R, S (Pico simulator) | Lucid: barometer | – |
| `timer_v1` | F401, F405 | F401: H, I, N, O, P, R | Foxeer: motor timers, outputs off | complementary outputs |
| `timer_v3` | H743, H753 | H753: I, J, P, Q (CHxN on TIM1) | Lucid: motor timers | – |
| `iwdg_v1` | F401, F405 | F401: J | Foxeer: started and fed | – |
| `iwdg_v2` | H743, H753 | H753: K (window, freeze, LSI) | – | – |
| `exti_v1` | F401, F405 | F401: E, F, G | Foxeer: IMU data ready | – |
| `exti_h7` | H743, H753 | H753: F, G, H | Lucid: IMU data ready | – |
| `dma_v2` | F401, F405 | F401: UART and SPI runs, all 9 USART and 10 SPI routes; I, O, P | – | double buffering, FIFO threshold |
| `dma_v1` + DMAMUX1 | H743, H753 | H753: all 16 DMA1/DMA2 streams; J, Q | – | double buffering, FIFO threshold |
| `gpio_v2` | all four | every Nucleo run | – | – |
| `adc_v2` | F401, F405 | F401: M | Foxeer: battery inputs | – |
| `adc_v4` | H743, H753 | H753: O | Lucid: battery inputs | – |
| `rcc_f4` | F401, F405 | F401: every run, HSE bypass | Foxeer: HSE crystal to 168 MHz | HSE crystal |
| `rcc_h7_rm0433` | H743, H753 | H753: every run, HSE bypass; I (voltage scale) | Lucid: HSE crystal to 400 MHz | HSE crystal |
| `sdmmc_v2` | H743, H753 | – | Lucid: a card, the empty slot | SDMMC-003, SDMMC-004, error paths |
| USB, F4 OTG_FS core | F401, F405 | – | Foxeer: CDC echo | USB-F4-001 |
| USB, H7 OTG core at FS | H743, H753 | H753: M (CN13) | Lucid: CDC echo | – |

## Shared engines and system

| what | proven on: runs | not proven |
|---|---|---|
| UART engine (`uart/stream_dma.rs`), both IP versions | the UART runs on both Nucleos | UARTDMA-009 (a stream that will not stop) |
| SPI engines, DMA and polled | the SPI runs on both Nucleos | – |
| Waveform generator, two-way lanes | F401: O, P, R; H753: Q | – |
| Reset cause, ROM bootloader request | F401: L; H753: N | RESET-004 on both families |
| UID and die revision | every run's closing log line | – |
| Caches and MPU shapes (H7) | H753: X; A to E with caches on | the SPI runs with caches on |
| Constructor refusals | F401: V; H753: U; both smoke benches | – |
| Injected faults | F401: W; H753: W | – |
| No reachable panic, glue against metapac, host tests | `checks/run.sh`, no board | USB inside the proof |

## Chip glue

One chip proves a family's shared glue; the other chip's own rows are checked against
stm32-metapac by `tools/check_glue.py` (127 facts on the F401, 173 on the F405, 180 on
the H743, 184 on the H753).

| family | proven on | rows on glue check only |
|---|---|---|
| F4 | F401: every pin entry and DMA route of its three USARTs and three SPIs | the F405's USART3, UART4/5, TIM6-8, TIM12-14 |
| H7 | H753: 7 of 8 U(S)ARTs, 24 of 46 TX/RX entries (Zio only), SPI1-5 on Zio pins, all 16 streams | UART8 TX, the morpho-only pins, the H743's TFBGA100 rows (pinned by a test) |
