# Scope: what FerroHAL is and is not

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's sections 1, 10, 12 and 13, dissolved 2026-11-10. Tags:
**[Decided]** settled by the owner, follow it; **[Proposal]** current
direction, raise it before building on it.

## Identity (was section 1)

- **[Decided]** FerroHAL is a general-purpose STM32 HAL usable from RTIC, async, bare metal or an equivalent. It is RTIC-first, but RTIC is never required.
- **[Decided]** Crate name `ferrohal-stm32`, in its own repository, independent of FerroForge and FerroWasp.
- **[Decided]** It is built only on `stm32-metapac`. Scope is limited to what metapac covers; anything metapac does not support is out of scope.
- **[Decided]** `embassy-stm32` is a reference to port from, never a dependency. Keep MIT/Apache attribution for ported code.
- **[Decided]** The "toaster rule": FerroHAL does not know whether it runs in a toaster or an aircraft. It contains no safety-tier, certification-class or FerroWasp-specific logic.
- **[Decided]** FerroHAL's main goals are: scheduler agnostic; no panic; a single unified HAL; requirement tracing and verification. These are the roots its requirements trace to ([goals](../../requirements/goals.md)); it does not trace to FerroWasp's W1-W7.

## Code generator, parked (was section 10)

**[Decided]** The generator is not part of the current work. Avoid closing it off:

- keep board facts as data, not code;
- keep routing traits typed per IP and per instance;
- remember that the metapac pin also pins the metadata a generator would read.

## Hard rules for the agent (was section 13)

FerroHAL must not:

- depend on FerroForge, FerroWasp or embassy-stm32;
- define interrupt handlers, set NVIC priorities or enable vectors in the NVIC;
- use critical sections in drivers, or write a shared register outside its single owner;
- contain async code or own a time source;
- panic on a hardware error, or spin without a bound in a stop path;
- choose DMA, pins or timers automatically;
- expose metapac through the safe public API;
- know about safety tiers;
- add the code generator now;
- modify RTIC.

The prototype is not intended for safety certification (was section 11). Section
12's deferred items are gone: USB and SDMMC shipped (the USB glue is
`synopsys-usb-otg`), and the one-transport-at-a-time migration was superseded by
[ADR-005](../decisions/2026-10-31-ferrowasp-integration.md)'s all-at-once swap.
