# Ownership and shared registers

Source: ADR-006 (archived: [`../decisions/archive/2026/2026-11-10-doc-map-dissolution.md`](../decisions/archive/2026/2026-11-10-doc-map-dissolution.md)).


The UART's one owner is its own
topic: [uart-ownership](uart-ownership.md).

- Ownership follows the svd2rust model: one `Peripherals::take()`, owned zero-sized tokens, and constrain or split to divide them further. metapac has no ownership model, so FerroHAL generates an svd2rust-style `Peripherals` per chip from metapac metadata. Tokens keep an `unsafe steal()`.
- Design rule: runtime operations only write registers the driver owns.
- Writes to shared registers (RCC enable and reset bits, GPIO mode, EXTI, SYSCFG, DMAMUX) go through the single owner of that shared block, for example `&mut Rcc` as in stm32f4xx-hal. This keeps them race-free on every core without bit-banding or critical sections.
  - An RTIC or bare-metal firmware does these writes in init.
  - A layer that constructs drivers at runtime (for example an async layer) wraps the owner in its own lock.
  - Bit-banding exists only on M4 parts (F4, G4) and is at most an optimisation. M7 and M33 parts (F7, H7, H5) have none.
- There are no critical sections inside drivers.
- One exception to the two rules above, and to the scope's NVIC rule: the jump to the ROM bootloader (`reset::enter_bootloader_if_requested`). It masks interrupts, turns SysTick off, disables and unpends every NVIC line, moves VTOR, and on the F4 writes RCC's SYSCFG enable and SYSCFG's memory remap without `&mut Rcc` or a SYSCFG part. It is sound because the jump is one-way: it runs first in `main` or `init`, before any owner exists, and nothing runs after it to observe or race the writes. No other path may cite it.
- Every driver has one owner, every operation takes `&mut self`, and concurrency is the scheduler's lock; interrupt service is `Service::on_interrupt` on that owner (ADR-011, superseding ADR-004's separate control and flag capabilities); design: [uart-ownership](uart-ownership.md).
- Register access is policy, not an escape hatch: [../policies/safety.md](../policies/safety.md) (G5 - no public `unsafe fn`, metapac unreachable from the public API, imports confined, every `unsafe` documented and machine-checked).
- Instance typing: an `Instance` trait checks routing at construction, after which the instance type is erased. This is the pattern used by embassy-stm32 (a static `Info` reference) and imxrt-hal (`AnyInstance`). One driver type then serves every instance (USART1 to USART6), so one FerroForge task can name a single concrete type.
