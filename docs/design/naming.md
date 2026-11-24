# Naming: catalogs, IDs, features, modules, benches

Source: ADR-021, [`../decisions/2026-11-24-naming-scheme.md`](../decisions/2026-11-24-naming-scheme.md).

How FerroHAL names what it is made of.
`tools/check_names.py` holds the rules that a checker can hold (in `checks/run.sh`).

## Requirement catalogs

- One catalog per IP or subsystem, all in `src/requirements/<ip>.yaml`.
  A catalog holds a family and its sub-categories: `uart.yaml` holds `UART`, `UART-DMA`,
  `USART-V2` and `USART-V4`; `timer.yaml` holds `TIMER`, `TIMER-PWM` and `TIMER-LANES`.
  `check_names.py` lists each family's catalog and holds every family to exactly one
  (per IP, not a file per family).

## Requirement IDs

- `<FAMILY>-<NNN>`: an upper-case family, three digits, no project prefix
  (the crate names the project: [`requirements-ids.md`](requirements-ids.md)).
- The family is the driver or subsystem. A sub-category takes the family, a hyphen
  and its own name, never run together with the family:
  - a driver kind: `UART-DMA-001`, `SPI-POLL-001`, `TIMER-PWM-001`, `TIMER-LANES-001`;
  - an IP version: `USART-V2-001`, `SPI-V4-001`, `I2C-V1-001`, `IWDG-V2-001`;
  - a chip family: `RESET-H7-001`, `USB-F4-001`;
  - a CPU core: `WAIT-M4-001`, `WAIT-M7-001`.
- What a chip's glue supplies is the glue's: the unique ID and the die revision are
  `GLUE-011` and `GLUE-012`, not families of their own.
- One family per prefix: `RESOURCE-001`, not `RES-001` beside `RESET-*`;
  the USART IP's own requirements are `UART-016` to `UART-020`, not a third UART
  prefix.

## Features

- `<ip>_v<N>` where stm32-metapac versions the IP (its number, not a
  chronology), `<ip>_<family>` where FerroHAL needs a family shape, and the RCC by its
  reference manual where one family has several shapes: `rcc_h7_rm0433` covers only the
  H742/743/750/753.
- metapac numbers DMA backwards: the F4's stream DMA is `dma_v2`, the H7's
  `dma_v1`. The numbers are metapac's, not an order.
- A chip feature is metapac's chip name and turns on every IP version the
  chip carries, through its line's private feature (`_line-stm32f401`); every part of a
  line carries the same versions, so the table names one part per line:

| chip | family | usart | spi | i2c | exti | timer | iwdg | dma | gpio | rcc | adc | sdmmc | USB core |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `stm32f401re` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f410rb` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | - |
| `stm32f411ce` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f405rg` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f412zg` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f413zh` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f427vi` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f429zi` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f446re` | F4 | v2 | v2 | v1 | v1 | v1 | v1 | v2 | v2 | `rcc_f4` | v2 | - | OTG_FS |
| `stm32f722re` | F7 | v3 | v3 | v2 | v1 | v1 | v2 | v2 | v2 | `rcc_f7` | v2 | - | OTG_FS |
| `stm32f745vg` | F7 | v3 | v3 | v2 | v1 | v1 | v2 | v2 | v2 | `rcc_f7` | v2 | - | OTG_FS |
| `stm32f767zi` | F7 | v3 | v3 | v2 | v1 | v1 | v2 | v2 | v2 | `rcc_f7` | v2 | - | OTG_FS |
| `stm32h743vi` | H7 | v4 | v4 | v2 | h7 | v3 | v2 | v1 | v2 | `rcc_h7_rm0433` | v4 | v2 | OTG_HS (FS) |
| `stm32h753zi` | H7 | v4 | v4 | v2 | h7 | v3 | v2 | v1 | v2 | `rcc_h7_rm0433` | v4 | v2 | OTG_HS (FS) |

usart v3: the F7's, served by the `usart_v4` module (ADR-011).

Which IP versions each series carries, as metapac measured them:
[`../notes/2026-11-10-ip-versions.md`](../notes/2026-11-10-ip-versions.md).

## Modules

- A neutral module plus a directory per IP version for every new IP:
  `uart` with `usart_v2`/`usart_v4`, and so `spi`, `i2c`, `adc`.
  A directory holding only `mod.rs` is that rule, not a leftover: it is where the IP
  version's next file goes.
- One folder per IP (ADR-012): every peripheral IP lives
  in its own folder, never as a loose file in `src/`, and its submodules live inside it.
  The folder is the unit that may later become its own crate (`ferrohal-<ip>`, per
  [structure.md](structure.md)), so it reaches only the crate's core (`resource`, `wait`,
  `time`, `rcc`, the chip's glue traits) and other IPs' public items, never a
  neighbour's internals. Loose files in `src/` are that core, not IPs.
- Deliberate exceptions: one source compiled under two version names, by
  `#[path = "<ip>/mod.rs"]` (`dma/` as `dma_v1`/`dma_v2`, `exti/` as `exti_v1`/`exti_h7`,
  `iwdg/` as `iwdg_v1`/`iwdg_v2`, `timer/` as `timer_v1`/`timer_v3`). Each is two IP
  versions, so each needs a chip per version. A file mounted by `#[path]` owns its
  folder, so `timer/mod.rs`'s `mod pwm;` finds `timer/pwm.rs` under both names. This
  list is complete: these four are every `#[path]` alias in `src/lib.rs` (the chip files'
  `#[path]`s pull in glue, not drivers). Not a driver alias either: the
  F4 clock tree's configuration and solver (`rcc_f4/tree.rs`), mounted by `rcc_f4` and by
  `rcc_f7`, which keeps its own register writes.
- A die with two DMA IPs (the H7: stream DMA, BDMA, MDMA) gives each its own
  folder and neutral name, as metapac does (`dma`, `bdma`), not one name for both.
- The RCC's shared code is a neutral module like `uart/`: `rcc/` holds what
  every RCC version shares (`kernel.rs`, `shared.rs`) and re-exports the chip's version
  (`rcc_f4`, `rcc_f7`, `rcc_h7_rm0433`).
- Not an exception: a neutral module's engine serving every IP version
  through that version's register wrapper (`Unit`, the UART's too since N7)
  (`uart/stream_dma/`, `spi/stream_dma.rs`,
  `spi/polled.rs`, `i2c/mod.rs`, `adc/mod.rs`). That is the neutral-module pattern
  itself, so these are never counted as shared sources.

## Vocabulary

- A hardware thing takes the name the first source gives it, in the order
  stm32-metapac, ST's reference manual, embassy-stm32, FerroHAL. Where a source names
  it differently between chips (F4, F7, H7, G4), or has no name for it, the next
  source decides, so a thing has one name on every chip. Code yields to this: that the
  code already says something is no reason to keep it.
- Kept against that order: *IP* and *IP version*, where metapac says "peripheral
  version".
- FerroHAL's own terms. *lane*: a DMA-fed timer output. *handle*: an ownerless view
  of a running driver that another context, an interrupt handler above all, holds and
  copies freely, because all it does is one register read, or one register write
  complete in itself: `ExtiHandle`, `TimebaseHandle` (from `Timebase::handle`),
  `Watchdog::handle`. The DMA's `FlagHandle` follows it. The UART has no handle since
  its single-owner redesign (ADR-011).
- Constructors are `new`; `Watchdog::start` is the deliberate exception,
  because an independent watchdog cannot be stopped once started and the name says so.
  Its token is `chip::IWDG` on every chip (the H7's IWDG1).
- `set_*` changes a setting in place; `into_*` consumes and converts.
  Units ride in the name: `*_us`, `*_hz`.
- One quantity, one word: *frequency*
  is a clock's (the I2C bus's SCL, SPI's SCK, the SDMMC card clock, a timebase's count);
  *rate* is events a second (a PWM's periods, a tick, a sampler's samples); *baudrate*
  stays the UART's, as a `Hertz`; *speed* is GPIO slew only.
- A refusal's alias is `<What>Refused`, named after what refuses, not
  after the method: `SpiRefused`, `I2cRefused`,
  `LaneRefused`, `SamplerRefused`, `ScanRefused`. Names stay put when a method is
  renamed. The UART's are `UartRefused` and `UartRxRefused` (ADR-011).
- No module prefix on an item, `spi::Parts` not `spi::SpiParts`, except: each module's main driver types (`Spi`, `I2c`,
  `Adc`, `AdcScan`, `UsbFs`, `Uart`) and the refusals named after them (`SpiRefused`); and the
  routing traits a chip's glue file imports beside another module's of the same name
  (`SpiTxDma` and `SpiRxDma` beside the UART's `TxDma`, `AdcPin` beside `gpio::Pin`).
  The UART's parts trait is `uart::Parts` (ADR-011).

- A clock tree's host tests sit in the chip file whose limits they need
  (`src/chips/<family>/<line>.rs`), and are named by the tree they pin, not by a board:
  `solve_gives_168_mhz_and_usb_from_an_8_mhz_crystal`. A comment may say which bench
  board freezes that tree.

## Benches and scripts

- `hil/run-<bench>.sh` runs a bench: `run-f401`, `run-h753`, `run-foxeer`,
  `run-lucid`. Helpers that are not benches take their own names: `flash-dfu.sh`,
  `pico-i2c-sim.sh`.
- A bench binary is named by its test, `src/bin/usb_cdc.rs` building
  `usb-cdc`, the same name on every bench that runs the test. The run letters stay as each bench's aliases, upper case in prose (run M)
  and lower case on the command line (`hil/run-h753.sh m`); `hil/<crate>/runs.tsv` maps
  them, and the run scripts and `tools/` read it. A letter still means a different test
  on each bench; the binary's name says which.
- A bench generates its runs only where several runs differ only in data:
  the H753's runs A to D (`tools/gen_h753_runs.py`), which open different UARTs at
  different rates on one template. Every other run, on every bench, is written by hand,
  because it tests something of its own.

## Documents

- Registries in capitals (`DEBT`, `README`, `WIRING`, `AGENTS`);
  everything else kebab-case, with dated names where
  `docs/policies/documentation.md` asks for them. The resolved-debt registry is a
  folder, `docs/resolved/`, whose hub is its `README.md` and whose parts, split for the
  size budget, are kebab-case.
