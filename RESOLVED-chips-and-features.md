# FerroHAL: resolved debt, by chip and feature

The chip and feature sections of [RESOLVED.md](RESOLVED.md), split out on 2026-11-13 to
keep it in its budget; the crate-wide section stays there. Newest first within each
section.

## STM32F401RE

- **HSE in bypass runs on hardware.** Run A freezes from the ST-LINK's 8 MHz MCO (UM1724 7.9.1, MB1136 C-02 and later) to the same 84 MHz tree; USART2's host echo confirms it against the host's clock.
- **Clock limits per chip:** each F4 chip file has its own `CLOCK_LIMITS`.
- **Every route runs on hardware:** all three USARTs on every TX/RX pin and DMA stream RM0368 lists.
- **Errata review** (ES0299 Rev 6, all revisions): 2.2.7 RCC enable delay handled (`Rcc::enable` reads back); 2.2.11, 2.10.1 and 2.10.2 tracked in DEBT.md.
- **Docs:** RM0368, DS10086, ES0299 and UM1724 in `docs/`; RM0368 tables 28 and 29 match the glue. ES0222 is for the F401xB/C and doesn't apply.

## STM32F405RG

- **Values checked against the docs** (RM0090 Rev 22, DS8626 Rev 12, ES0182 Rev 19 in `docs/stm32f405/`):
  - DS8626: HCLK 168 MHz (VOS 1; 144 MHz with VOS 0), PCLK1 42, PCLK2 84; HSE crystal 4–26 MHz, external 1–50 MHz; PLL input 0.95–2.10 MHz; VCO 100–432 MHz.
  - RM0090: PLLM 2–63, PLLN 50–432; wait states (table 11) 30 MHz per WS at 2.7–3.6 V, 5 WS for 150–168 MHz; VOS is PWR_CR bit 14, reset value 1 (scale 1); SRAM1 + SRAM2 at 0x2000_0000 for all AHB masters, CCM at 0x1000_0000 CPU-only; 0.5 and 1.5 stop bits unavailable on UART4/5 (30.6.5).
- **UART4/5 reserved bit.** SR bit 9 (CTS) is reserved on UART4/5 (RM0090 30.6.1); `clear_tc` wrote 1 there and now writes 0, which is also right on the USARTs since CTS is only set with CTSE on.
- **Errata review** (ES0182): 2.2.13 RCC enable delay handled; 2.2.1 (prefetch, rev A), 2.2.19 (DMA2 AHB/APB2), 2.11.1 (IDLE), 2.11.2 (PE) tracked in DEBT.md. Not applicable: 2.2.11 (APB slowed during DMA: clocks are set once, before any driver), 2.6.1 (DAC), 2.11.3–2.11.8 (mute mode, CTS, smartcard, NACK, guard time, CTS with break: none used).
- **UART4 and UART5.** metapac 21 types them as `usart::Usart`; 0.5 and 1.5 stop bits are refused per instance (`Instance::FRACTIONAL_STOP_BITS`) by `Uart::new`, `UartRx::new`, `set_config` and `Uart::check`; no run uses UART4/5, so it is inspection only (DEBT.md §8).
- **CCM RAM is refused for DMA:** `check_dma_buffer` accepts SRAM1 and SRAM2 only (0x2000_0000, 128 KiB).
- **Runs on hardware** (2026-11-02, `hil/run-foxeer.sh`, 28 of 28 and the host's echo up to 64 KiB, at 561 KiB/s), on the Foxeer F405 V2 over its USB port, flashed through the ROM DFU bootloader:
  - **HSE with a crystal:** 8 MHz to 168 MHz. RCC agrees, and the CPU counts 167.9 million cycles per 1000 host USB frames, the one clock not from the crystal.
  - **The SPI NOR on the real chip:** Winbond JEDEC ID `ef 40 18` at 10.5 MHz over `BlockingSpi`. Erase (47 ms), 16 pages programmed (slowest 379 µs), read back, erased again, only in FerroWasp's scratch sector; the sectors either side unchanged.
  - **The IMU path:** ICM-42688-P WHO_AM_I over SPI1 on DMA2 streams 3 and 0, then FerroWasp's setup: 405 data-ready edges in 200 ms on EXTI4, 400 bursts of 15 bytes at 10.5 MHz started from them (slowest 24 µs, none failed or missed), the accelerometer reading 993 mg at rest.

## Restarting into the ROM bootloader

- **`reset::reboot_to_bootloader`, `bootloader_requested`, `enter_bootloader`, `peek_cause`** (2026-11-02), for the F4 and H7, with the chips' system memory addresses and the F4's memory remap in the glue. Run L on the F401, run N on the H753ZI: the request survives a software reset, with the data cache off and on, and answers yes once. The Foxeer's bench enters its ROM DFU through it.
- **Found on the Lucid:** a single word written to AXI SRAM just before a reset is lost (RM0433 2.4; ECC per 64 bits). The bench measured a lone word, a barrier and a read back all lost; a whole doubleword or a further write kept. The F4's RAM has no ECC, so its benches never showed it.

## SD cards

- **`sdmmc::Sdmmc`** (2026-11-02), on `sdmmc_v2`: a 16 GB SanDisk in the Lucid, at 25 MHz on 4 bits, reads and writes one block and sixteen in one command (16 blocks: about 5 ms to write, 0.9 ms to read), and has the blocks it borrowed restored. A read waits for the end of data, where a CRC error on the last block shows.

## USB from its interrupt

- **`OTG_FS` under RTIC at priority 5** (2026-11-02), FerroWasp's arrangement, on the Foxeer and the Lucid: the host's echo up to 64 KiB, at the polled rate. The firmware services USB only from that vector.
- **VTOR after a DFU `leave`:** the F405's ROM jumps to the application with no reset, leaving VTOR on the ROM, so the firmware's first interrupts ran the ROM's USB handler. The control run proved it: the same firmware without `set-vtor` gets STALLed descriptor reads and `unable to enumerate`; with it, it echoes up to 64 KiB. FerroHAL's `rt` feature now turns `set-vtor` on, for every firmware.
- **`reset::enter_bootloader` hands the ROM a clean core:** SysTick off, every NVIC line disabled and cleared, interrupts unmasked. RTIC's `init` runs masked with its lines enabled, and the ROM's USB needs interrupts: without this the board came up in the ROM but never enumerated.

## ADC and unique ID

- **`adc::Adc` and `AdcScan`** (2026-11-02), on `adc_v2` and `adc_v4`: ADC1 and, on the H7, ADC3, read one at a time or as a sequence over DMA. Run M on the F401, run O on the H753ZI, and the Foxeer's battery inputs in FerroWasp's sequence (temperature, voltage, current; 77 µs at the slowest). Physical checks: both rails over the SPI links, VDDA 3302 to 3333 mV and the internal reference 1202 to 1222 mV against factory calibration, the die at 27 to 36 °C.
- **`signature::device_uid`:** the unique ID, in memory order.
- **Glue:** `check_glue.py` checks each ADC's enable bit, pins and DMA routes, plus each SPI's kernel clock, SYSCFG's enable, DMA stream vectors, the watchdog peripheral and EXTI vectors, the gaps it had before.

## One timer starting another

- **`Pwm::lead` and `Pwm::follow`** (2026-11-02): FerroWasp's Foxeer starts TIM8 from TIM1 so all four lanes begin on one edge. Run N on the F401 (TIM1, TIM3), run P on the H753ZI and the Foxeer's own check (TIM1, TIM8): the slave waits, starts every time over 100 starts, and reads within 3 or 4 counts of the master, read one after the other. No output is enabled in any of them.
- **TIM3 starts TIM2 on the F4s** (2026-11-06): the SpeedyBee F405 V5's motors sit on TIM3 CH3/CH4 (PB0/PB1) and TIM2 CH3/CH4 (PB10/PB11), so its bank needs TIM3 leading TIM2 the way the Foxeer's TIM1 leads TIM8. The glue gained `timer_trigger!(TIM2: TIM3, 2)` on the F401 and the F405 (RM0090 table 94, RM0368 table 54: TIM2's ITR2). Run R on the F401 drives the SpeedyBee's four lanes at the DShot600 rate from one start — TIM2 in software beside TIM3, then from TIM3's trigger, then every lane active low as bidirectional DShot idles the line — all read back by the TIM1/DMA2 port-B sampler, 18 checks. It rides TIM2 CH2 on PB3 for the second TIM2 lane: the board's TIM2 CH4 pin, PB11, is not bonded on the F401's LQFP64 (DS10086's pin table), and the lane's DMA route, stream 6 channel 3, is the same one CH4 uses. `trigger_suite`'s first check now asks only that the slave is not running, not that its counter reads zero — a stopped timer keeps its count, and run R's slave arrives having run. Run N is unchanged and still passes.

## STM32H753ZI

- **HSE in bypass runs on hardware.** Run A, the Lucid mirror, freezes from the ST-LINK's 8 MHz MCO (UM2407 7.5.1): 8 MHz reference, 800 MHz wide VCO, 400 MHz core.
- **The D-cache can be on.** With it on, `check_dma_buffer` requires each buffer to lie wholly in one enabled MPU region with non-cacheable attributes (TEX, C, B; subregions honoured) that no higher region overlaps. The HIL turns both caches on with buffers in a non-cacheable `DMA_NC` block, and checks that cached RAM and the DTCM are refused.
- **Errata covered:** ES0392 2.20.4 (USART DMA lock-up; TRBUFF set on every UART transfer), 2.5.4 (DMAMUX routing; SE stays 0, SPOL 00).

## STM32H743VI

- **Values checked against DS12110 Rev 11** (`docs/stm32h743/`): HSE crystal 4–48 MHz, external 4–50 MHz; wide VCO 192–836 MHz (rev Y, table 40; 960 MHz rev V, table 137), medium 150–420 MHz; PLL input 2–16 MHz wide, 1–2 MHz medium; 400/300/200 MHz in scales 1/2/3. All match `src/chips/shared/h7.rs`.
