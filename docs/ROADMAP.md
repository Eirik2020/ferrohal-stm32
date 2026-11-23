# Not supported yet

The roadmap: features FerroHAL does not offer. None is debt; the owner sets the order.

- **Chips:** the STM32F411, F722, F745 and G474, in progress: [the plan](plans/2026-11-17-f411-f722-f745.md).
- **UART:** 7- and 9-bit data, RTS/CTS, half-duplex, RS-485 DE, LIN; LPUART1; the
  H7's hardware FIFOs.
- **SPI:** slave, simplex, half-duplex, 16-bit frames, hardware NSS, CRC; SPI6.
- **I2C:** slave, 10-bit addresses, SMBus; `InterruptI2c` on `i2c_v2` (F7, H7); DMA on the
  H7's I2C4 (BDMA only).
- **Timers:** input capture, dead time, the break input, trigger routes beyond those in
  use: among them TIM5 from TIM3 on the H743 and H753 (RM0433 table 341, ITR2), so a
  generator on TIM3 and TIM5 starts both in software there. A lane reader paced by a
  compare channel of a timer that is not a lane timer (`docs/design/lanes.md`: the
  paced form uses that timer's update request only).
- **Time adapters** (allowed by G1, behind features; `policies/features.md`): a FerroHAL
  timer as rtic-monotonics' monotonic (rtic-time), an embassy-time driver.
- **EXTI:** event mode, the internal lines (PVD, RTC, USB wake-up).
- **GPIO:** an open-drain output type, runtime mode switching; `free()` returning pins.
- **DMA:** BDMA, MDMA.
- **ADC:** timer triggers, injected channels, analog watchdogs, oversampling,
  differential inputs, ADC2, the H7's `_C` pads, per-channel sample times.
- **USB:** OTG_HS at high speed, host mode.
- **SDMMC:** DMA, card detect, above 25 MHz, 1.8 V; SDIO on the F4.
- **Clocks and power:** other supply ranges on the F401; the H7's VOS0 (480 MHz, rev V),
  the SMPS, PLL2/PLL3, fractional mode, CSI, HSIDIV other than 1, TIMPRE, other kernel
  muxes, the I/O compensation cell, CRS trimming of HSI48.
- **Memory:** a cache-maintenance mode for DMA buffers in cacheable memory.
- **Drivers:** flash, WWDG, CAN/FDCAN, DAC, RNG, CRC, RTC, SAI, Ethernet, QSPI, FMC,
  LTDC.
