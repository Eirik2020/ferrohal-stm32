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

Which IP versions each series carries, as metapac measured them:
[`../notes/2026-11-10-ip-versions.md`](../notes/2026-11-10-ip-versions.md).

## Modules

- **[Decided]** A neutral module plus a directory per IP version for every new IP:
  `uart` with `usart_v2`/`usart_v4`, and so `spi`, `i2c`, `adc`.
  A directory holding only `mod.rs` is that rule, not a leftover: it is where the IP
  version's next file goes. The timer's `timer_pwm.rs` and `timer_awg.rs` stay children
  of `timer.rs` by `#[path]`, because `timer.rs` is itself compiled as `timer_v1` and
  `timer_v3` (the shared sources below). API review S2 closed on this (owner,
  2026-11-17).
- **[Decided]** Deliberate exceptions: one source compiled under two names
  (`iwdg.rs` as `iwdg_v1`/`iwdg_v2`, `exti.rs`, `dma_stream.rs`), and the timer split
  by function, not version (`timer.rs`, `timer_pwm.rs`, `timer_awg.rs`, as
  `timer_v1`/`timer_v3`). Each is two IP versions, so each needs a chip per version.
  This list is complete: these four are every `#[path]` alias in `src/lib.rs` and
  `src/timer.rs` (the chip files' `#[path]`s pull in glue, not drivers). ADR-003 rule 4
  named only the first and the timer; the 2026-11-13 debt sweep counted five. Both
  counts are superseded by this one (API review K4, 2026-11-17).
- **[Decided]** Not an exception: a neutral module's engine serving every IP version
  through that version's register wrapper (`Unit`; the UART's `LineRegs`, N7)
  (`uart/stream_dma.rs`, `spi/stream_dma.rs`,
  `spi/polled.rs`, `i2c/mod.rs`, `adc/mod.rs`). That is the neutral-module pattern
  itself, so these are never counted as shared sources.

## Vocabulary

- **[Decided]** *channel*: a timer compare output. *lane*: a DMA-fed output. *stream*:
  a DMA stream. *line*: a UART's control registers. *handle*: an ownerless view of a
  running driver that another context, an interrupt handler above all, holds and copies
  freely, because everything it does is a single read or a write no one else races:
  `ExtiHandle`, `TimebaseHandle` (from `Timebase::handle`), `Watchdog::handle` (owner,
  2026-11-17, API review N5). The DMA's `FlagHandle` and the UART's `UartIrq` follow it
  with the UART's single-owner redesign.
- **[Decided]** Constructors are `new`; `Watchdog::start` is the deliberate exception,
  because an independent watchdog cannot be stopped once started and the name says so.
  Its token is `chip::IWDG` on every chip (the H7's IWDG1) (owner, 2026-11-17, N17).
- **[Decided]** `set_*` changes a setting in place; `into_*` consumes and converts.
  Units ride in the name: `*_us`, `*_hz`.
- **[Decided]** One quantity, one word (owner, 2026-11-17, API review N3): *frequency*
  is a clock's (the I2C bus's SCL, SPI's SCK, the SDMMC card clock, a timebase's count);
  *rate* is events a second (a PWM's periods, a tick, a sampler's samples); *baudrate*
  stays the UART's, as a `Hertz`; *speed* is GPIO slew only.
- **[Decided]** A refusal's alias is `<What>Refused`, named after what refuses, not
  after the method (owner, 2026-11-17, API review N1): `SpiRefused`, `I2cRefused`,
  `LaneRefused`, `SamplerRefused`, `ScanRefused`. Names stay put when a method is
  renamed. The UART's comes with its single-owner redesign.
- **[Decided]** No module prefix on an item, `spi::Parts` not `spi::SpiParts` (owner,
  2026-11-17, API review N2), except: each module's main driver types (`Spi`, `I2c`,
  `Adc`, `AdcScan`, `UsbFs`) and the refusals named after them (`SpiRefused`); and the
  routing traits a chip's glue file imports beside another module's of the same name
  (`SpiTxDma` and `SpiRxDma` beside the UART's `TxDma`, `AdcPin` beside `gpio::Pin`).
  The UART's names come with its single-owner redesign.

- **[Decided]** A clock tree's host tests sit in the chip file whose limits they need
  (`src/chips/<part>.rs`), and are named by the tree they pin, not by a board:
  `solve_gives_168_mhz_and_usb_from_an_8_mhz_crystal`. A comment may say which bench
  board freezes that tree (API review S5, 2026-11-17).

## Benches and scripts

- **[Decided]** `hil/run-<bench>.sh` runs a bench: `run-f401`, `run-h753`, `run-foxeer`,
  `run-lucid`. Helpers that are not benches take their own names: `flash-dfu.sh`,
  `pico-i2c-sim.sh` (owner, 2026-11-13).
- **[Decided]** Run letters upper case in prose (run M), lower case in file names
  (`run_m.rs`, `run-m`).
- **[Decided]** A bench generates its runs only where several runs differ only in data:
  the H753's runs A to D (`tools/gen_h753_runs.py`), which open different UARTs at
  different rates on one template. Every other run, on every bench, is written by hand,
  because it tests something of its own (owner, 2026-11-17, API review B2).

## Documents

- **[Decided]** Registries in capitals (`DEBT`, `RESOLVED`, `README`, `WIRING`,
  `AGENTS`); everything else kebab-case, with dated names where
  `docs/policies/documentation.md` asks for them.
