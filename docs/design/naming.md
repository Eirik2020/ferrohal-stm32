# Naming: catalogs, IDs, features, modules, benches

Source: ADR-003, [`../decisions/2026-11-08-naming-scheme.md`](../decisions/2026-11-08-naming-scheme.md).

How FerroHAL names what it is made of. Promoted from ADR-003 when W14 applied it
(2026-11-13); the owner settled the four choices it left open the same day.
`tools/check_names.py` holds the rules that a checker can hold (in `checks/run.sh`).

## Requirement catalogs

- **[Decided]** One catalog per IP or subsystem, all in `src/requirements/<ip>.yaml`.
  A catalog holds a family and its part families: `uart.yaml` holds `UART`, `UARTDMA`,
  `USART-V2` and `USART-V4`; `signature.yaml` holds `UID` and `REVISION`.
  `check_names.py` lists each family's catalog and holds every family to exactly one
  (owner, 2026-11-13: per IP, not a file per family).

## Requirement IDs

- **[Decided]** `<FAMILY>-<NNN>`: an upper-case family, three digits, no project prefix
  (the crate names the project: [`requirements-ids.md`](requirements-ids.md)).
- **[Decided]** A requirement only one IP version or chip family has takes the family, a
  hyphen and the part: `USART-V2-001`, `SPI-V4-001`, `I2C-V1-001`, `IWDG-V2-001`,
  `RESET-H7-001`, `USB-F4-001` (owner, 2026-11-13). A driver kind is not a part and
  stays one word: `UARTDMA`, `SPIPOLL`.
- **[Decided]** One family per prefix: `RESOURCE-001`, not `RES-001` beside `RESET-*`;
  the USART IP's own requirements are `UART-016` to `UART-020`, not a third UART
  prefix.

## Features

- **[Decided]** `<ip>_v<N>` where stm32-metapac versions the IP (its number, not a
  chronology), `<ip>_<family>` where FerroHAL needs a family shape, and the RCC by its
  reference manual where one family has several shapes: `rcc_h7_rm0433` covers only the
  H742/743/750/753 (owner, 2026-11-13).
- **[Decided]** metapac numbers DMA backwards: the F4's stream DMA is `dma_v2`, the H7's
  `dma_v1`. The numbers are metapac's, not an order.
- **[Decided]** A chip feature is metapac's chip name and turns on every IP version the
  chip carries:

| chip | family | usart | spi | i2c | exti | timer | iwdg | dma | gpio | rcc | adc | sdmmc | USB core |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `stm32f401re` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f405rg` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32h743vi` | H7 | v4 | v4 | v2 | h7 | v3 | v2 | v1 | v2 | `rcc_h7_rm0433` | v4 | v2 | OTG_HS (FS) |
| `stm32h753zi` | H7 | v4 | v4 | v2 | h7 | v3 | v2 | v1 | v2 | `rcc_h7_rm0433` | v4 | v2 | OTG_HS (FS) |

## Modules

- **[Decided]** A neutral module plus a directory per IP version for every new IP:
  `uart` with `usart_v2`/`usart_v4`, and so `spi`, `i2c`, `adc`.
- **[Decided]** Deliberate exceptions: one source compiled under two names
  (`iwdg.rs` as `iwdg_v1`/`iwdg_v2`, `exti.rs`, `dma_stream.rs`), and the timer split
  by function, not version (`timer.rs`, `timer_pwm.rs`, `timer_awg.rs`, as
  `timer_v1`/`timer_v3`). Each is two IP versions, so each needs a chip per version.

## Vocabulary

- **[Decided]** *channel*: a timer compare output. *lane*: a DMA-fed output. *stream*:
  a DMA stream. *line*: a UART's control registers. *handle*: an interrupt or health
  side of a driver.
- **[Decided]** `set_*` changes a setting in place; `into_*` consumes and converts.
  Units ride in the name: `*_us`, `*_hz`.

## Benches and scripts

- **[Decided]** `hil/run-<bench>.sh` runs a bench: `run-f401`, `run-h753`, `run-foxeer`,
  `run-lucid`. Helpers that are not benches take their own names: `flash-dfu.sh`,
  `pico-i2c-sim.sh` (owner, 2026-11-13).
- **[Decided]** Run letters upper case in prose (run M), lower case in file names
  (`run_m.rs`, `run-m`).

## Documents

- **[Decided]** Registries in capitals (`DEBT`, `RESOLVED`, `README`, `WIRING`,
  `AGENTS`); everything else kebab-case, with dated names where
  `docs/policies/documentation.md` asks for them.
