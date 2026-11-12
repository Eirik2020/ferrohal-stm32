# Ownership / `free()` redesign

Next project after the sweep, its own PR. `steal()` goes under G5; firmware needs a second
role. `Refused` returns tokens on failure; `free()` is the other half. Precedent:
h7xx-hal 0.16 has 11 `free(...)` (`free(self) -> ($peripheral, $rec)`); f4xx-hal 0.23 none.

## Blockers (proven)

- `Pwm<T>` (`src/timer_pwm.rs:131`) keeps `PhantomData<T>` only; `PwmChannel` (`:366`) and `CompareLane` (`:613`) hold register blocks and no borrow -> a safe `Pwm::free()` could run while a lane drives the timer. Lifetime-binding conflicts with `start`/`stage` taking `&mut Pwm` while lanes live -> split handle or lifetime redesign.
- ~~`Tick`/`Timebase` non-generic, token erased (`hil/common/timers.rs:22`, `:68`)~~
  **done (2026-11-10):** `Tick<T>`/`Timebase<T>` carry the token; the bench's
  `TickBench<E>` holds a per-run enum of benched ticks so the token stays sealed while
  the vector runs.
- ~~`I2c` (`src/i2c/mod.rs:256`), `Spi`, `Uart` store `&'static Info`, not tokens -> store ZST tokens.~~
  **Uart done (2026-11-11):** the token rides in `Line<U>`. **Spi and I2c done (2026-11-12):**
  `Spi<U>`, `BlockingSpi<U>` and `I2c<U>` keep the token; `free()` stops, closes and
  returns it.
- ~~`UartRx`/`UartTx` need a rejoin before `Uart::free()`.~~ **done (2026-11-11):**
  `split`/`join` pair added; `free()` takes the whole `Uart` (or the line, for a
  receive-only `UartRx::free`).
- ~~A split UART's `UartRx::free(rx, line)` returned the token under a live `UartTx`,
  and halves crossed instances~~ **done (2026-11-13):** `UartTx<U>`/`UartRx<U>`;
  receive-only `RxLine<U>`; probes in `checks/no-metapac`.

## Scope

1. Generic handles where needed; `free(self) -> T`, stopping as `Drop` would; document.
   **Tick/Timebase done 2026-11-10** (own PR): `Tick<T>`/`Timebase<T>` carry the token,
   `free()` stops the timer and hands it back; the bench's `TickBench<E>` keeps the
   token sealed in a per-run enum of benched ticks, `Link` reads through a
   `TimebaseReader`. **UART, SPI, I2C done 2026-11-11/12.** Remaining: none in this
   item.
2. Lifetime-bind `PwmChannel`/`CompareLane`/`TwoWayLane` (or split `Timer` from `Pwm`);
   `Pwm::free()`. **Done 2026-11-10** (own PR): channels borrow the handle; lanes hold
   no timer access and take a `&Pwm` per compare write, so the `Awg` keeps owning both.
3. `Uart::free()` via half rejoin; `Spi::free()`; `I2c::free()`. **Uart done 2026-11-11**
   (this PR): `Uart<U>` keeps the token in its `Line<U>` handle; `split()` ->
   `(Line<U>, UartTx, UartRx)` and `join()` back; `free()` stops both streams, closes
   the line and returns the token; a receive-only `UartRx::free(self, line)` returns it
   too. The bench's `Link`/`Loop`/`HostEcho`/`Cross`/`Pair` are generic over the
   instances; `Line::set_config`/`send_break` borrow the transmit half. **Spi and I2c
   done 2026-11-12** (this PR): `Spi<U>`, `BlockingSpi<U>` and `I2c<U>` keep the token
   in the handle; `free()` stops the streams (SPI) or closes the unit (I2C, polled SPI)
   and returns it; `new`'s `check` moved the instance to the type (`Spi::<SPI1>::check`);
   `open`'s refusals hand the token back with the wiring. The SPI/EXTI bench's `Bus<U>`
   holds one instance's handle, one `Bus` per instance in the run binaries.
4. DMA stream/channel handles: **decided 2026-11-12 — they stay transport-owned.**
   A `Channel` erases the stream token into a controller and a number for the
   drivers' use, so it cannot hand a typed token back; `Channel::start` is
   `unsafe` and internal since W6b2; and every driver that claims streams stops
   and releases them in its own `free()`. If a standalone DMA handle is ever
   offered, it takes its own `free()` then. Recorded in `docs/design/free.md`.
5. ~~Migrate the six second-use bench checks back to token reuse~~ **done
   2026-11-12:** `tick_check` returns the benched tick; runs H (F401) and I
   (H753) free TIM4 and reopen it as the 8 kHz control tick and free TIM2 and
   TIM5 to reopen them as 1 MHz timebases, all measured as before. Fresh-boot
   runs U (F401) and T (H753) are deleted, off the run scripts' defaults and
   out of `requirements/benches.md`.
6. ~~Update `DEBT.md` (pointer) and driver module docs~~ **done 2026-11-12:**
   DEBT's "`Spi` and `I2c` have no `free()`" moved to RESOLVED; `free.md` and
   the teardown brief record the landed shapes and the DMA decision.

## W14 naming

Moved to its own plan, [`2026-11-08-w14-naming.md`](2026-11-08-w14-naming.md), on
2026-11-13: not started, and not part of this work's PR.

## Gates

`checks/run.sh` **passes 2026-11-13** (no `check_names` step: that is W14's). Both
Nucleo suites ran green at this branch's code the same day: F401 21 runs / 318 checks,
H753 21 runs / 532 checks; the Foxeer (49) and the Lucid (42, card fitted) through DFU
and USB. reqtool, recorded at `08e2c25`: **118 verified, 0 stale, 2 unverified** —
`SDMMC-003` wants a Lucid run with the slot empty, and `UARTDMA-009` has no method (its
DEBT item stands). H753 run M (CN13 cable) was not rerun. Evidence:
`requirements/evidence/logs/free-redesign-20261113/`.
FerroWasp pins `b6d8e84`; its bump meets `Spi::<SPIx>::check` (five board asserts).
