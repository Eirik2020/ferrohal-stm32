# FerroHAL: supported silicon and capabilities

What FerroHAL supports on each chip, the limits a user must design around, and what it
does not support yet. Open work on what exists is [DEBT.md](DEBT.md); what each
capability's proof is, bench by bench, is [`hil/COVERAGE.md`](../hil/COVERAGE.md).
IP versions and features per chip: [`design/naming.md`](design/naming.md#features).

## Silicon

| part | series, core | die | package (bonded pins) | clock as offered | revision on the rig | bench (tier) |
|---|---|---|---|---|---|---|
| STM32F401RE | F4, Cortex-M4F | 0x433 | LQFP64 | 84 MHz | 0x1001 | Nucleo-F401RE (`HIL`) |
| STM32F405RG | F4, Cortex-M4F | 0x413 | LQFP64 | 168 MHz | 0x100f | Foxeer F405 V2 (`FLSH`) |
| STM32H743VI | H7, Cortex-M7 | 0x450 | TFBGA100 (as LQFP100) | 400 MHz (VOS1) | 0x2003, V | TBS Lucid H7 (`FLSH`) |
| STM32H753ZI | H7, Cortex-M7 | 0x450 | LQFP144 | 400 MHz (VOS1) | 0x2003, V | Nucleo-H753ZI (`HIL`) |

Maturity follows three axes, IP version, die and series, on the tier ladder `BRD`,
`HIL`, `DBG`, `FLSH`, `INH` ([`design/maturity.md`](design/maturity.md)). A capability is as
strong as its weakest axis; one not listed here is not covered. The computed tier
tables replace this paragraph when the tier command lands (ADR-009). The silicon and
capability tables here are hand-written until the generator writes them (ADR-008, item 7);
then they are generated and must not be edited by hand.

## Capabilities

✓ supported · – on the chip, not supported · n/a not on the chip.

| capability | F401RE | F405RG | H743VI | H753ZI |
|---|---|---|---|---|
| **UART** over DMA, full duplex, idle-ended chunks | USART1/2/6 | USART1/2/3/6, UART4/5 | USART1/2/3/6, UART4/5/7/8 | as H743 |
| receive-only, parity, break, line masking, `free()` | ✓ | ✓ | ✓ | ✓ |
| RX/TX inversion (refused on `usart_v2`, USART-V2-001) | n/a | n/a | ✓ | ✓ |
| LPUART | n/a | n/a | – | – |
| **SPI** master over DMA, and polled (`BlockingSpi`) | SPI1-3 | SPI1-3 | SPI1-4 | SPI1-5 |
| SPI6 (BDMA) | n/a | n/a | – | – |
| **I2C** master, 7-bit, polled | I2C1-3 | I2C1-3 | I2C1-4 | I2C1-4 |
| **Timers**: `Tick`, `Timebase`, PWM, one-pulse, trigger start | TIM1-5, 9-11 | TIM1-14 | TIM1-8, 12-17 | as H743 |
| compare lanes and samplers over DMA, the waveform generator | ✓ | ✓ | ✓ | ✓ |
| complementary outputs (CHxN) | TIM1 | TIM1, TIM8 | TIM1, 8, 15-17 | as H743 |
| LPTIM, HRTIM | n/a | n/a | – | – |
| **EXTI**, GPIO lines 0-15 | ✓ | ✓ | ✓ | ✓ |
| **GPIO** per-package bonded pins, alternate functions | ✓ | ✓ | ✓ | ✓ |
| **DMA** streams (DMA1, DMA2); DMAMUX1 on the H7 | ✓ | ✓ | ✓ | ✓ |
| BDMA, MDMA | n/a | n/a | – | – |
| **ADC** regular sequence, software start, over DMA | ADC1 | ADC1 | ADC1, ADC3 | ADC1, ADC3 |
| internal channels: VREFINT, die temperature, factory calibration | ✓ | ✓ | ✓ | ✓ |
| **Watchdog** (IWDG): start, LSI-scaled, debug freeze | ✓ | ✓ | ✓ | ✓ |
| windowed watchdog start | n/a | n/a | ✓ | ✓ |
| **Clocks** (`freeze`, const solver): HSI, HSE bypass, HSE crystal, PLL | ✓ | ✓ | ✓ | ✓ |
| voltage scale | ✓ | ✓ | VOS1 | VOS1 |
| **USB** device CDC, full speed | OTG_FS | OTG_FS | OTG (FS) | OTG (FS) |
| **SDMMC**, 4-bit, polled | n/a (SDIO: –) | n/a (SDIO: –) | SDMMC1 | SDMMC1 |
| **System**: reset cause, ROM bootloader entry, UID, die revision | ✓ | ✓ | ✓ | ✓ |
| **Adapters** (features): `fugit` rate conversions | ✓ | ✓ | ✓ | ✓ |
| tokens only, no driver: CAN/FDCAN, DAC, RNG, CRC, RTC, WWDG, SAI, ETH, QSPI, FMC, LTDC | – | – | – | – |

## Limits

Silicon behaviour and documented contracts a user designs around; none is a bug. Each
names where the code documents it.

**UART**
- `usart_v2`: clearing a line error reads SR then DR, so one byte may be lost per
  reported error (`Uart`; v4 clears through ICR).
- `usart_v2`: parity detection is reliable only while the receiving USART is not
  transmitting (ES0299 2.10.2, ES0182 2.11.2; `Uart`).
- `usart_v2`: IDLE is missed when the sender's clock runs faster than the receiver's,
  so an idle-ended `read_chunk` needs a timeout fallback (ES0299 2.10.1, ES0182 2.11.1;
  `Uart`).
- Frames: 8 data bits, parity as a ninth.
- A transmit DMA error is reported after the frame was queued: in `TxHealth`, and from
  the next `write` or `flush`.
- `write_count` gives up after `COUNT_TRIES` if interrupts keep landing between its reads,
  and uses the last sample.
- The receive ring must be served within one ring's worth of bytes, and reading code
  must never preempt `UartIrq` (`Buffers::rx`).
- Line-error positions are approximate: the mark sits where DMA had written; more than
  16 between reads report without a mark.
- `verify()` checks the vectors are enabled at one priority, not that it is above every
  task using the UART.

**Waits**
- Counted-wait floors are measurements (UART `RX_PASS_CYCLES` 224, `TX_PASS_CYCLES` 48;
  SPI `PASS_CYCLES` 112). Without a `CycleClock`, UART waits run 2.5-7 times long and
  SPI waits up to 3 (F401) or 6 (uncached H753). A new chip must pass the bench's
  wait-bound check.

**SPI**
- `BlockingSpi` holds the CPU for the whole transfer, one byte in flight, with gaps of
  1.5-1.6 times the wire time.

**I2C**
- A line held low is `Error::Busy`, not repaired: nine-clock recovery is the caller's.

**Timers**
- A `Tick` or `Timebase` rate must divide the timer clock exactly (`ConfigError::Inexact`).
- Some timers share their update vector (F4 `TIM1_UP_TIM10`, H7 `TIM8_UP_TIM13`); the
  firmware calls every sharer's handler.
- Arming and staging two lanes takes 14 µs on the F401 at 84 MHz.
- Samplers are measured up to about 7 MHz.
- Only the trigger routes in use are offered: TIM1→TIM3, TIM1→TIM8, and TIM3→TIM2 on
  the F4 (RM0090 table 94, RM0368 table 54, RM0433 table 336).
- F4: DMA2 serving GPIO and APB2 at once can corrupt data (ES0299 2.2.11, ES0182
  2.2.19), and only DMA2 reaches GPIO, so samplers and two-way lanes must not overlap
  APB2 DMA (TIM1, TIM8, SPI1, USART1/6).

**Watchdog**
- The LSI drifts with temperature and supply; `start_with_lsi` scales to one measured
  constant, and `start_windowed` counts at a nominal 32 kHz.

**ADC**
- H7: the ADC runs at a fixed 4 MHz (per_ck over 16), BOOST at reset; ES0392 2.9.8
  limits ADC3 while the LSE runs. One sample time serves every channel.

**DMA, memory and caches (H7)**
- DMA1/DMA2 cannot reach the DTCM: DMA buffers belong in AXI SRAM (`chip::DMA_RAM`).
- `check_dma_buffer` steps the MPU's RNR and restores it: do not reprogram the MPU while a
  `Uart` opens. Buffers must be in non-cacheable memory.
- DMAMUX overruns are not watched; ES0392 2.5.1 says CFR writes do not raise them.

**Clocks and power (H7)**
- The wide VCO is capped at 836 MHz (revision Y) though V allows 960.
- LDO supply only; PWR_CR3 is written once per power-on reset, so a locked other
  configuration gives `SupplyTimeout`.
- Kernel muxes are forced: USART16910SEL PCLK2, USART234578SEL PCLK1, SPI123SEL PLL1Q,
  SPI45SEL PCLK2, so SPI1-3 need PLL1's Q.
- HSI48 runs untrimmed for USB; CRS trimming fails on revision Y (ES0392 2.2.3).
- Flash wait states are transcribed from RM0433 table 17, checked once.
- A future flash driver must follow ES0392 2.2.6, 2.2.7 and 2.2.18
  (`design/h7-errata.md`).

**SDMMC**
- Polled through the FIFO, up to 25 MHz, 4-bit, 3.3 V, high-capacity v2 cards.

**USB**
- The `usb` feature can panic: `synopsys-usb-otg` has about 30 panic and unwrap sites and
  unbounded register spins. It is G2's listed exception, off by default
  (`policies/features.md`).

## Not supported yet

The roadmap: features FerroHAL does not offer. None is debt; the owner sets the order.

- **UART:** 7- and 9-bit data, RTS/CTS, half-duplex, RS-485 DE, LIN; LPUART1; the
  H7's hardware FIFOs.
- **SPI:** slave, simplex, half-duplex, 16-bit frames, hardware NSS, CRC; SPI6.
- **I2C:** slave, 10-bit addresses, SMBus, DMA.
- **Timers:** input capture, dead time, the break input, trigger routes beyond those in
  use.
- **Time adapters** (allowed by G1, behind features; `policies/features.md`): a FerroHAL
  timer as rtic-monotonics' monotonic (rtic-time), an embassy-time driver.
- **EXTI:** event mode, the internal lines (PVD, RTC, USB wake-up).
- **GPIO:** an open-drain output type, runtime mode switching; `free()` returning pins.
- **DMA:** BDMA, MDMA.
- **ADC:** timer triggers, injected channels, analog watchdogs, oversampling,
  differential inputs, ADC2, the H7's `_C` pads, per-channel sample times.
- **USB:** OTG_HS at high speed, host mode; a panic-free device core, retiring the `usb`
  feature's G2 exception.
- **SDMMC:** DMA, card detect, above 25 MHz, 1.8 V; SDIO on the F4.
- **Clocks and power:** other supply ranges on the F401; the H7's VOS0 (480 MHz, rev V),
  the SMPS, PLL2/PLL3, fractional mode, CSI, HSIDIV other than 1, TIMPRE, other kernel
  muxes, the I/O compensation cell, CRS trimming of HSI48.
- **Memory:** a cache-maintenance mode for DMA buffers in cacheable memory.
- **Drivers:** flash, WWDG, CAN/FDCAN, DAC, RNG, CRC, RTC, SAI, Ethernet, QSPI, FMC,
  LTDC.
