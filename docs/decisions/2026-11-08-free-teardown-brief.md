# Decision: ownership and `free()` (the teardown brief, 2026-11-08)

ADR-002 - Status: accepted

Scheduled 2026-11-08, after the debt sweep. Plan:
PRs #35 to #38 (the plan file was deleted on 2026-11-13; git history keeps it).

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
4. The naming normalisation (W14) travels with this project, not the sweep. (Went
   otherwise: W14 became its own PR, #39, 2026-11-13.)

## Proven blockers (do not rediscover)

- `Pwm<T>` (`src/timer_pwm.rs:131`) keeps only `PhantomData<T>`; `PwmChannel<T, C>`
  (`:366`) and `CompareLane<T, C>` (`:613`) hold register blocks and no borrow, so a safe
  `Pwm::free()` would be callable while a lane still drives the timer. Binding them by
  lifetime conflicts with `start`/`stage` taking `&mut Pwm` while lanes live - hence a
  redesign, not a method.
- ~~`Tick`/`Timebase` are **non-generic**; the token is erased~~ **done (2026-11-10):**
  they are `Tick<T>`/`Timebase<T>` and hand the token back with `free()`; the bench's
  `TickBench<E>` holds a per-run enum of benched ticks so the token stays sealed while
  the vector runs.
- ~~`I2c` (`src/i2c/mod.rs:256`) and `Spi`/`Uart` store `&'static Info`, not their tokens.~~
  **done (2026-11-11/12):** all three carry the token in the handle and hand it back
  with `free()`.
- ~~`UartRx`/`UartTx` must rejoin before `Uart::free()`.~~ **done (2026-11-11):**
  `split`/`join` pair; `free()` takes the whole `Uart`. Since 2026-11-13 the halves
  carry their instance and a receive-only UART frees through its own `RxLine`
  ([`../design/free-shapes.md`](../design/free-shapes.md)).

## Consequences

- Every affected file's evidence is re-owed (architecture §9 hashes).
- The fresh-boot bench runs stay valid; they simply become redundant and can be folded
  back once `free()` exists.
- `DEBT.md`'s "no `release()`/`free()`" entry closes only when the redesign lands
  (closed 2026-11-12, moved to [RESOLVED.md](../RESOLVED.md)).
