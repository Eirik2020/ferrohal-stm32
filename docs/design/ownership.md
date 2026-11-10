# Ownership and shared registers

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 4, dissolved 2026-11-10. The UART's control/flag
split is its own topic: [uart-ownership](uart-ownership.md).

- **[Decided]** Ownership follows the svd2rust model: one `Peripherals::take()`, owned zero-sized tokens, and constrain or split to divide them further. metapac has no ownership model, so FerroHAL generates an svd2rust-style `Peripherals` per chip from metapac metadata. Tokens keep an `unsafe steal()`.
- **[Decided]** Design rule: runtime operations only write registers the driver owns.
- **[Decided]** Writes to shared registers (RCC enable and reset bits, GPIO mode, EXTI, SYSCFG, DMAMUX) go through the single owner of that shared block, for example `&mut Rcc` as in stm32f4xx-hal. This keeps them race-free on every core without bit-banding or critical sections.
  - An RTIC or bare-metal firmware does these writes in init.
  - A layer that constructs drivers at runtime (for example an async layer) wraps the owner in its own lock.
  - Bit-banding exists only on M4 parts (F4, G4) and is at most an optimisation. M7 and M33 parts (F7, H7, H5) have none.
- **[Decided]** There are no critical sections inside drivers.
- **[Decided]** Control and flag registers are separate capabilities; design: [uart-ownership](uart-ownership.md).
- **[Decided]** Register access is policy, not an escape hatch: [../policies/safety.md](../policies/safety.md) (G5 - no public `unsafe fn`, metapac unreachable from the public API, imports confined, every `unsafe` documented and machine-checked).
- **[Decided]** Instance typing: an `Instance` trait checks routing at construction, after which the instance type is erased. This is the pattern used by embassy-stm32 (a static `Info` reference) and imxrt-hal (`AnyInstance`). One driver type then serves every instance (USART1 to USART6), so one FerroForge task can name a single concrete type.
