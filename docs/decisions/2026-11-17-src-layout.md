# ADR-012: The `src/` layout, clean slate (2026-11-17)

ADR-012 - Status: promoted

Promoted to: [naming.md](../design/naming.md), [structure.md](../design/structure.md).

## Context

A clean-slate review of `src/` chose the best long-term layout, keeping one folder per
IP: a folder is the unit that may later become its own crate when more families share
it. Further out, FerroHAL may reach other vendors (NXP, RP, Microchip), with the neutral
top-level API (`uart`, `spi`, `i2c`, `adc`, ...) moved into its own crate so a user sees
one API on every MCU; so the neutral modules stay thin and vendor-agnostic in shape, and
the IP folders stay self-contained. What did not hold up:

- `timer_pwm.rs` and `timer_awg.rs` sat beside `timer.rs` because that file is compiled
  under two names (`timer_v1`, `timer_v3`). That reason is wrong: a file mounted by
  `#[path = "timer/mod.rs"]` owns its folder, so `mod pwm;` finds `timer/pwm.rs` under
  both names (tested in a scratch crate, then in this crate on all five chips).
- Five IPs were loose files in `src/` (`dma_stream.rs`, `exti.rs`, `iwdg.rs`,
  `syscfg.rs`, `usb.rs`) beside the crate's core files, so `src/` mixed IPs and core.
- RCC's version-neutral code sat in loose root files, and `chips/` was flat: about 20
  line files and 40 shared files once the planned dies land.

## Decision

1. One folder per IP, holding its submodules; loose files in `src/` are the crate's
   core only. A folder reaches only that core and other IPs' public items, so it can be
   cut into a crate. The timer is `src/timer/{mod,pwm,lane_group}.rs`; the DMA
   `src/dma/mod.rs`, mounted as `dma_v1`/`dma_v2`; `exti/`, `iwdg/`, `syscfg/`, `usb/`.
2. A second DMA IP on one die (BDMA on the H7 and G474) gets its own folder and neutral
   name (`bdma`), as metapac names it.
3. `src/rcc/` is a neutral module: `kernel.rs` and `shared.rs`, then the chip's version
   (`rcc_f4`, `rcc_h7_rm0433`) re-exported, as `uart/` does for `usart_vN`.
4. `src/chips/` groups by family (`f4/`, `f7/`, `h7/`, later `g4/`): shared facts and
   macros, dies, errata tables and one chip file per line, named by the lines it serves.
   What spans families sits in `chips/`. `chips/gen/` stays flat. The glue generator
   emits the chip selection that was hand-written in `lib.rs`.

## Rejected

- A neutral `dma` module organised by engine (`dma::stream`, later `dma::channel`): it
  breaks one folder per IP, and with it the crate cut. Separate names per IP solve the
  H7 as well.
- Nesting IP-version folders under their neutral module (`spi/v2/`): the sibling rule
  stays, and alphabetical order already groups `spi`, `spi_v2`, `spi_v4`.
- Grouping the core files (`resource`, `wait`, `time`, ...) into a folder: about ten
  files, and a folder buys nothing.

## Consequences

Moving a file re-owes its bench evidence, because records key on path and hash.
