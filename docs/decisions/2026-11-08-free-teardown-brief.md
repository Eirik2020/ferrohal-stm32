# Decision: ownership and `free()` (the teardown brief, 2026-11-08)

ADR-002 - Status: accepted

Scheduled 2026-11-08, after the debt sweep. Plan:
[`../plans/2026-11-08-free-redesign.md`](../plans/2026-11-08-free-redesign.md).

## Context

G5 removes `steal()`, but firmware legitimately needs one peripheral in a second role
(a timer as `Tick`, then as `Pwm`; a UART polled, then with DMA). `Refused` already
returns tokens when a constructor refuses, so `free()` is the missing half of the same
contract. Ecosystem precedent exists: `stm32h7xx-hal-0.16` has 11 `free(...)` methods
(`free(self) -> ($peripheral, $rec)`); `stm32f4xx-hal-0.23` has none.

## Decision

1. Handles own their tokens and can hand them back (`free()`), across the drivers.
2. Sub-handles are lifetime-bound or joinable: `PwmChannel`/`CompareLane`/`TwoWayLane`
   borrow their timer (or a split timer handle), `UartRx`/`UartTx` rejoin into `Uart`.
3. Until then, the six bench "second use" checks move into fresh-boot runs so
   `tokens!::steal` can be deleted in the debt sweep (W4b).
4. The naming normalisation (W14) travels with this project, not the sweep.

## Proven blockers (do not rediscover)

- `Pwm<T>` (`src/timer_pwm.rs:131`) keeps only `PhantomData<T>`; `PwmChannel<T, C>`
  (`:366`) and `CompareLane<T, C>` (`:613`) hold register blocks and no borrow, so a safe
  `Pwm::free()` would be callable while a lane still drives the timer. Binding them by
  lifetime conflicts with `start`/`stage` taking `&mut Pwm` while lanes live - hence a
  redesign, not a method.
- `Tick`/`Timebase` are **non-generic**; the token is erased (`hil/common/timers.rs:22`,
  `tick_check(..., tick: Tick, ...)` at `:68`). They need `Tick<T>`/`Timebase<T>`.
- `I2c` (`src/i2c/mod.rs:256`) and `Spi`/`Uart` store `&'static Info`, not their tokens.
- `UartRx`/`UartTx` must rejoin before `Uart::free()`.

## Consequences

- Every affected file's evidence is re-owed (architecture §9 hashes).
- The fresh-boot bench runs stay valid; they simply become redundant and can be folded
  back once `free()` exists.
- `DEBT.md`'s "no `release()`/`free()`" entry closes only when the redesign lands.
