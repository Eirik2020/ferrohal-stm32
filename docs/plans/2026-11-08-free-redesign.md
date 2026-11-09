# Ownership / `free()` redesign (with W14 naming)

Next project after the sweep, its own PR. `steal()` goes under G5; firmware needs a second
role. `Refused` returns tokens on failure; `free()` is the other half. Precedent:
h7xx-hal 0.16 has 11 `free(...)` (`free(self) -> ($peripheral, $rec)`); f4xx-hal 0.23 none.

## Blockers (proven)

- `Pwm<T>` (`src/timer_pwm.rs:131`) keeps `PhantomData<T>` only; `PwmChannel` (`:366`) and `CompareLane` (`:613`) hold register blocks and no borrow -> a safe `Pwm::free()` could run while a lane drives the timer. Lifetime-binding conflicts with `start`/`stage` taking `&mut Pwm` while lanes live -> split handle or lifetime redesign.
- `Tick`/`Timebase` non-generic, token erased (`hil/common/timers.rs:22`, `:68`) -> must become `Tick<T>`/`Timebase<T>`.
- `I2c` (`src/i2c/mod.rs:256`), `Spi`, `Uart` store `&'static Info`, not tokens -> store ZST tokens.
- `UartRx`/`UartTx` need a rejoin before `Uart::free()`.

## Scope

1. Generic handles where needed; `free(self) -> T`, stopping as `Drop` would; document.
2. Lifetime-bind `PwmChannel`/`CompareLane`/`TwoWayLane` (or split `Timer` from `Pwm`); `Pwm::free()`.
3. `Uart::free()` via half rejoin; `Spi::free()`; `I2c::free()`.
4. DMA stream/channel handles: `free()` or stay transport-owned - decide.
5. Migrate the six second-use bench checks back to token reuse (fresh-boot runs stay valid).
6. Update `DEBT.md` (pointer) and driver module docs.

## W14 naming (this PR)

Findings: `../notes/2026-11-08-naming-audit.md`; decision: `../decisions/2026-11-08-naming-scheme.md`.

- Catalogs: one family/file, central `src/requirements/<family>.yaml`; split `platform.yaml` -> rcc/gpio/dma/wait/resource; SIG -> `signature.yaml`; GLUE -> `glue.yaml`; move the four co-located catalogs in.
- IDs: RES-001 -> RESOURCE-001; SIG-001 -> UID-001; merge USART-* into UART-* (keep USARTV4-*/SPIV2-*); UARTDMA-* stays.
- Features: `rcc_h7rm0433` -> `rcc_h7_rm0433`; document inverted `dma_v1`/`dma_v2` (F4=v2, H7=v1) and the chip->feature table in architecture §2.
- Modules: neutral + per-IP dirs for new IPs; document `iwdg.rs` and `timer*.rs` as exceptions.
- Vocabulary: channel/lane/stream/line/handle; `set_*` vs `into_*`; `*_us`/`*_hz`.
- Scripts `run-<bench>.sh`; letters uppercase in prose, lowercase in files.
- Docs: caps for registries, kebab for prose.
- `tools/check_names.py` in `checks/run.sh`: every `Requirements:` ID resolves once; no duplicates; catalog name matches family; feature names match pattern (would have caught USART-001).

## Gates

`checks/run.sh` (with `check_names`), reqtool, each driver's bench proof + full
`run-f401.sh`/`run-h753.sh`, ferro-wasp `cargo check` at the rev bump, CI green. Hash-based
evidence: plan the re-verification.
