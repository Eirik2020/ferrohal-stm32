# `free()`: handing tokens back (2026-11-09, landed 2026-11-13)

Every driver handle that owns a peripheral token hands it back with `free(self) -> T`,
stopping what it ran as a `Drop` would: requirement `API-002`, proven on both Nucleos
(runs V/U free each UART, SPI and I2C and read them off; the timer runs reopen theirs).
Driver by driver, what blocked each and the shape that landed:
[`free-shapes.md`](free-shapes.md).

Source: ADR-002, [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).

## The rules, and what meets them

1. Every handle that can touch a peripheral carries the token, or a borrow of it; a
   second handle is a compile error, not a runtime rule. **Met:** the token lives in
   `Pwm<T>`, `Tick<T>`, `Timebase<T>`, `Line<U>`/`RxLine<U>`, `Spi<U>`,
   `BlockingSpi<U>`, `I2c<U>` and `Sampler<T>`; an `Awg` frees both its timers;
   `PwmChannel` borrows its `Pwm`; the UART halves carry their instance.
2. `free(self) -> T` returns the token only when the last handle is gone. **Met:** a
   UART frees whole, after `join`, or receive-only through its `RxLine`; a channel
   borrows, so it is gone before `Pwm::free`.
3. A channel or lane that outlives `Pwm::free` must not be constructible. **Met:**
   `PwmChannel<'a, ..>` borrows; a `CompareLane` holds no timer access and writes
   only through a `&Pwm` it is handed.

Proof: `checks/no-metapac` probes that a split UART's receive half cannot free the
line and that halves do not cross instances (E0308); `checks/no-panic` runs every
`free()`, `split` and `join` on all four chips.

## Open

- No `free()` returns its pins, so a second role on the same pins is not
  reachable: only the instance token comes back (`DEBT.md`, API gaps).
- The DMA stream tokens stay transport-owned: a `Channel` erases the stream
  identity, so it cannot return one; every driver that claims streams stops and
  releases them in its own `free()`.

## Gate

W4b moved the six "second use" checks into fresh-boot runs (F401 run U, H753 run T) and
deleted `tokens!::steal`. With `free()` landed they folded back into runs H and I —
TIM4 reopens at 8 kHz, TIM2 and TIM5 as 1 MHz timebases, from the freed token — and
the fresh-boot runs are gone. `Peripherals::steal` stays the only unsafe hatch, W6
gated behind `rtic`.

Decisions: [`../decisions/2026-11-08-free-teardown-brief.md`](../decisions/2026-11-08-free-teardown-brief.md).
Plan: [`../plans/2026-11-08-free-redesign.md`](../plans/2026-11-08-free-redesign.md).
