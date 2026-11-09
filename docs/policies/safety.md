# Safety policy

Normative. Goal **G5** (`requirements/goals.md`). Rationale:
`../decisions/2026-11-08-g5-safe-by-default.md`. Evidence:
`../notes/2026-11-08-rtic-and-hal-comparison.md`.

## Rule

- Public API: no `unsafe fn`.
- Exception needs all three: written rationale, owner's dated approval, default-off feature gate.
- Need registers, raw pointers, a pre-setup jump? Add `stm32-metapac` yourself.

## Exception 1 - `Peripherals::steal()`

- `#[cfg(feature = "rtic")]`; `rtic = ["rt"]`; `default = []`; nothing else implies `rtic`.
- Item required: `rtic-macros-2.4.0` `module.rs:38` emits `Device::Peripherals::steal()` when `peripherals = true`.
- `unsafe` required by soundness, not RTIC: a safe steal aliases one peripheral twice and breaks exclusivity; the call sits in a generated `pub unsafe fn new` (`module.rs:294-299`).
- Safe alternative: `take()`. Internal path: `steal_unchecked()`.
- Metapac's own `steal()`/`from_ptr` on PAC types = "take the PAC", not a FerroHAL exception.
- `device` keeps `Interrupt`, `NVIC_PRIO_BITS`, `interrupt!`, `Peripherals`.

## Never public (crate-internal, no feature)

`DmaInstance::regs`, `DmaInstance::mux`, `PortInstance::regs`, SDMMC `Instance::regs`,
`Channel::regs`, uart stream `regs`, `Stream::steal`, `Channel::start(&Transfer)`,
`Channel::set_idle_buffer`, `reset::enter_bootloader` (public path `reboot_to_bootloader()`).

## Registers

- Only the token-owning module touches its registers.
- Metapac imports only in `src/chips/` + 12 drivers: usart_v2, usart_v4, spi_v2, spi_v4, i2c_v1, i2c_v2, adc_v2, adc_v4, gpio_v2, dma_stream, sdmmc_v2, iwdg.
- No register type over a `pub` boundary.
- Non-obvious writes cite document + section beside the code.
- Do not wrap register writes in `unsafe fn`: metapac `Reg` ops are safe and typed; `unsafe` means a caller memory-safety precondition.

## Enforce

- Deny `clippy::undocumented_unsafe_blocks`, `unsafe_op_in_unsafe_fn`.
- `checks/safe-api.py`: no `pub unsafe fn` outside chip glue beyond the allowlist; allowlist item must carry `#[cfg(feature = "rtic")]`; `default = []`, `rtic = ["rt"]`, nothing implies `rtic`; metapac imports confined.
- `checks/no-metapac`: compile-fail probes for the six accessors + a `ferrohal_stm32::pac` path.
- New public function -> add to `checks/no-panic`. Requirement **API-001**.

## Change

Owner approval + dated decision record + `requirements/goals.md` update. Never silently.
