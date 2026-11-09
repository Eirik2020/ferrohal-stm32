# `free()`: the teardown the drivers do not have (2026-11-09)

There is no `Pwm::free`, `Uart::free` or `Tick` hand-back. This is the brief for adding
them, and the reason the obvious safe signature is unsound today.

Source: ADR-002, [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).

## Why a safe `free()` is unsound now

- `Pwm<T>` holds `PhantomData<T>`: the token was moved in, and a `free(self) -> T` would
  hand back a `T` the handle never stores.
- `PwmChannel<T, C>` and `CompareLane<T, C>` hold neither the token nor a borrow of the
  PWM; after the PWM is freed, one can still write the same compare register through its
  own copy of the register block.
- `Tick` and `Timebase` are not generic: the token is erased when they are built, so there
  is nothing to return.

## What the redesign must establish

1. Every handle that can touch a peripheral carries the token, or a borrow of the handle
   that does; a second handle is a compile error, not a runtime rule.
2. `free(self) -> T` returns the token only when the last handle is gone.
3. A channel or lane that outlives `Pwm::free` must not be constructible.

## Gate

W4b removed the bench's need for second tokens: the six "second use" checks now run on a
fresh boot (F401 run U, H753 run T) and `tokens!::steal` is gone. `Peripherals::steal`
remains the only unsafe hatch, and W6 gates it behind `rtic`. No safe `free()` lands before
the ownership rules above, because today's shapes cannot express them.

Decisions: [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).
Plan: [`../plans/2026-11-08-free-redesign.md`](../plans/2026-11-08-free-redesign.md).
