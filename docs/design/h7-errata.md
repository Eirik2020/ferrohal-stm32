# ES0392 review: cache, system, flash ECC

W11 of the debt sweep. Read against ES0392 Rev 15 for the RM0433 chips
(H742/H743/H750/H753). A verdict is about FerroHAL's code: `HAL` means no driver
or glue path reaches the condition; `FW` means the HAL leaves it to firmware
(MPU, cache maintenance, SysTick source, WWDG scope, option bytes) and the note
says how; `DEBT` means DEBT.md carries it; `BOARD` means a package fact read from
the datasheet, with no firmware action and no run. FerroWasp: no cache, no MPU,
core SysTick.

Source: ADR-001, [`../decisions/2026-11-08-g5-safe-by-default.md`](../decisions/2026-11-08-g5-safe-by-default.md).

The verdicts below moved into the errata table on 2026-11-15 (ADR-010): every ES0392 Rev 15
row with its handling, generated into [`../errata/ES0392.md`](../errata/ES0392.md) from
`src/chips/shared/errata_es0392.rs`. Two verdicts changed on the way: 2.2.3 (CRS) is not
reached, since FerroHAL never uses the CRS, and 2.2.6/2.2.7 (flash ECC) are firmware rules;
the DEBT items this review named for them no longer existed.

Driver items sit where they act and cite their entry: SPI 2.22.2/5/6, SDMMC 2.8.4. USART
2.20.4, DMAMUX 2.5.4 and ADC 2.9.8 were listed here but cited nowhere; they are unreviewed
in the table (DEBT.md).
