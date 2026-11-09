# RTIC + reference-HAL evidence (G5)

Code-only, 2026-11-08/09. Sources: `rtic-macros-2.4.0`; `stm32f4xx-hal-0.23.0` +
`stm32f4-0.16.0`; `stm32h7xx-hal-0.16.0` + `stm32h7-0.15.1`; `stm32-metapac-21.0.0`.

## The steal exception

- `rtic-macros-2.4.0/src/codegen/module.rs:38` emits `device: #device::Peripherals::steal()` when `peripherals = true`; RTIC fixes path, name, signature.
- The call sits in a generated `pub unsafe fn new` (`:294-299`) -> RTIC needs the item, not the keyword.
- Soundness: a safe steal mints two token sets over one peripheral (two `Pwm<TIM1>` on one CCR; two `Uart` on one USART).
- Safe alternative: `peripherals = false` + `take()`. `take()` calls `steal()` internally (`src/chips/shared/macros.rs:63`) -> crate-private `steal_unchecked()`.

## Registers in the reference HALs

- f4xx-hal: `lib.rs:20-28` `pub use stm32f4::stm32f401 as pac`; PAC `Deref` at `stm32f4-0.16.0/src/generic.rs:40`; HAL adds `SysCfg -> SYSCFG` (`syscfg.rs:23`), `SAICH -> sai::CH` (`sai.rs:258`).
- h7xx-hal: `lib.rs:97+` `pub use stm32h7::stm32h743 as stm32`; PAC `impl Deref for TIM2` (`stm32h743/mod.rs:3730-3743`); HAL `inner()/inner_mut() -> &$peripheral` (`xspi:446/451`), `&WWDG` (`system_watchdog:108/113`), `&MAC`.
- metapac: 0 `Deref` impls; `pub const TIM2: timer::TimGp32 = unsafe { TimGp32::from_ptr(0x4000_0000) }` -> any re-export hands out safe register access.
- FerroHAL: private `use stm32_metapac as pac`; re-exports only `Interrupt`; tokens are its own ZSTs (`tokens!`, `macros.rs:11`), so `chip::TIM4` is not a register block.

## Unsafe counts

| crate | `pub unsafe fn` | gated |
|---|---|---|
| f4xx-hal 0.23 | 10: raw bit ops, `freeze_unchecked`, `release->(DWT,DCB)`, `stream()->&mut STREAM`, `next_transfer_with`, SAI `manual` | 0 |
| h7xx-hal 0.16 | 9: `get_stream()->&mut STREAM`, `new_unchecked`, `next_dbm_transfer_with`, `buf_as_slice_mut` x2, `enable_interrupt`/`interrupt_handler` | 0 |
| FerroHAL before sweep | 6 decls + 4 accessors + `Peripherals`/`tokens` steal | 0 |
| FerroHAL after sweep | 1 (`Peripherals::steal`) | `rtic` |

## Teardown

h7xx-hal 11 `free(...)` (`free(self) -> ($peripheral, $rec)`; DMA `free -> (STREAM, PERIPHERAL,
BUF, Option<BUF>)`); f4xx-hal 0. FerroHAL 0, but tokens return on refusal.

## Experiments (first in Act mode, `/tmp` crates)

| test | setup | expect | proves |
|---|---|---|---|
| T1 | current crate, `peripherals = true` | compiles | baseline |
| T2 | copy without `steal` | E0599 | RTIC needs the item |
| T3 | copy with a safe `pub fn steal` | compiles | keyword = FerroHAL soundness, not RTIC |

Results go into the G5 rationale and the PR body.

### Results (2026-11-09)

`rtic 2.4.0`, `cargo check --offline --target thumbv7em-none-eabihf`, crates in
`/tmp/rtic-t` (the two HAL copies differ only in `Peripherals::steal`):

| test | result |
|---|---|
| T1 | exit 0 |
| T2 | E0599: no associated function or constant named `steal` found for struct `Peripherals` |
| T3 | exit 0 |

The expansion needs the item, not the keyword. `pub unsafe fn new` (`:294-299`) calls
`steal()` in its body; T3's safe `pub fn steal` satisfies that call untouched, so
`unsafe` on ours states the aliasing rule, not a constraint RTIC imposes.
