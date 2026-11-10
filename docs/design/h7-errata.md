# ES0392 review: cache, system, flash ECC

W11 of the debt sweep. Read against ES0392 Rev 15 for the RM0433 chips
(H742/H743/H750/H753). A verdict is about FerroHAL's code: `HAL` means no driver
or glue path reaches the condition; `FW` means the HAL leaves it to firmware
(MPU, cache maintenance, SysTick source, WWDG scope, option bytes) and the note
says how; `DEBT` means DEBT.md carries it; `BOARD` means a package fact read from
the datasheet, with no firmware action and no run. FerroWasp: no cache, no MPU,
core SysTick.

Source: ADR-001, [`../decisions/2026-11-08-g5-safe-by-default.md`](../decisions/2026-11-08-g5-safe-by-default.md).

| § | Verdict | Note |
|---|---|---|
| 2.1.1 write-through D-cache | FW | No workaround: use write-back. `check_dma_buffer` refuses cacheable buffers; run X holds under a write-through region (benches-faults.md). |
| 2.1.2 PLD linefill | FW | Needs the MPU on with the default map off: keep `PRIVDEFENA` (the bench does) or map region 0 as 4 GB no-access. |
| 2.1.3 MBIST ICACHE | HAL | MBIST is never started. |
| 2.1.4 locked DEBRs | FW | Never lock DEBR0 and DEBR1 together. |
| 2.1.5 invalidate then store | FW | A DMB after any invalidate a store follows; `reboot_to_bootloader` cleans after its stores; run X holds with the DMB (benches-faults.md). |
| 2.2.1 CPU1/2-only watchdog reset | FW | The scope bit is the WWDG's and no WWDG driver exists; set `RCC_GCR.WW1RSC` first. IWDG1 resets the full domain (RM0433 §8.4.4). |
| 2.2.2 timer system breaks | FW | No break API yet; when one lands, system break sources do not reach TIM1/8 (the pin input does). |
| 2.2.3 CRS with USB SOF | DEBT | HSI48 runs untrimmed; already recorded. |
| 2.2.4 SysTick external clock | FW | `SystClkSource::External` is HCLK, not HCLK/8: select Core (rtic-monotonics 2.1 does) or multiply the reload by 8. |
| 2.2.6/2.2.7 flash ECC | DEBT | `FLASH_ECC_FAxR` is the only usable address; 2.2.7 has no workaround. |
| 2.2.5 option-byte waits, 2.2.8 PCROP, 2.2.9 bank swap, 2.2.17 RDP regressions, 2.2.18 flash CRC | FW | Provisioning: target wait states for an option-byte load, RDP 2 for PCROP, no bank swap, a spare backup-SRAM access before a second RDP regression, no flash CRC. |
| 2.2.10 AXI SRAM reads, rev Y/W | DEBT | DMA buffers are AXI SRAM here; `READ_ISS_OVERRIDE` caps read issuing at 1. |
| 2.2.22 VDDLDO on TFBGA100 | BOARD | The pin is not available on the TFBGA100 (the Lucid's VIH6): F4 is internally tied to VDD, so it is a VDD ball, not a rail (workaround none). Table 8 bonds the same GPIO set as LQFP100, so one `BONDED` table. |
| 2.1.6 FPU vector, 2.2.11/12/15/20/25/30 LSE/RTC, 2.2.21 480 MHz, 2.2.13 USB (with its module), 2.2.14 datasheet, 2.2.16 HRTIM, 2.2.19 DAC, 2.2.23 WWDG, 2.2.24 tamper, 2.2.26 `_C` pads, 2.2.27 Ethernet, 2.2.28 HASH, 2.2.29 DMA2D | HAL | No such driver or path; clock limits stay 400 MHz. |

Driver items sit where they act: SPI 2.22.2/5/6, USART 2.20.4, DMAMUX 2.5.4, SDMMC 2.8.4, ADC 2.9.8, CRS 2.2.3.
