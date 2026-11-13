# Duplication and reuse, U1-U11 (API review)

What repeats, with measured counts. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`. Counts
measured (`rg`, `diff`, `comm`); line savings not estimated.

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| U1 | M | Chip glue | H753: 545 of 593 distinct lines also in H743. F401: 367 of 478 also in F405. `ports!`, `usarts!`, `dma!` defined in all 4 chip files. Identical items: S1 |
| U2 | M | RCC | Identical in `rcc_f4` and `rcc_h7rm0433`: `Hse`, `HseMode`, `PllSource`, `AhbPrescaler`, `ApbPrescaler`, `RccBit`, private `wait`; reset-cause plumbing same shape |
| U3 | M | One-shot DMA transfer | `Spi`, `AdcScan`, `CompareLane` (arm + listen), `Sampler`: claim -> stop -> clear, `Transfer` literal, idle/running/done state, flag triage, `poll` alias. 8 literals outside `dma_stream.rs`, 7 carry `#[cfg(feature = "dma_v1")] bufferable`; 11 `claim_channel` calls |
| U4 | M | Lane assembly | 4 near-identical bodies: `timer_pwm.rs:424` `into_lane`, `:496` `assemble`, `:532` `assemble_two_way`, `:584` complementary `assemble`. `LaneWiring`/`ComplementaryLaneWiring` differ in the pin trait only |
| U5 | L | Resource rows | `Resource::Pin { .. }` x13 in 5 files; polled + DMA SPI wirings share 4 rows |
| U6 | L | SPI DMA vs polled | clock, divider, `sck_cycles`, bound, `HOLD_AFTER_END` handling twice (`spi/stream_dma.rs`, `spi/polled.rs`). `spi::Info` == `i2c::Info` (diff empty) |
| U7 | L | Buffer casts | 3 unsafe slice-to-bytes fns only to call `check_dma_buffer`: `adc/mod.rs` `as_bytes(&[u16])`, `timer_pwm.rs` `as_bytes(&[u32])`, `u16_bytes` |
| U8 | L | Constants | `STOP_SPINS` x4 (10 000 x3; 100 000 in `adc`); `PASS_CYCLES` 112 x2; `MARGIN_CYCLES` 20 000 x3 |
| U9 | L | Enable-then-reset | `rcc.enable(x); rcc.reset(x)` x11 |
| U10 | L | Vector check | `interrupt()` + `verify_vectors([v])` x3: `timer.rs:365`, `timer_pwm.rs:800`, `:1001` |
| U11 | L | Bench | `hil/*/src/bin/usb_irq.rs`: 3 of 101 lines differ (Foxeer vs Lucid) |

## Fix (opinion)

- U1: per-die facts in `chips/shared/`; chip file = tokens, `BONDED`, package pin rows. Owner 2026-11-11: after the routing generator (`DEBT.md` §1).
- U2: one shared RCC module; `bus_registers!` for both.
- U3: internal one-shot helper in `dma`; `Transfer` constructors; `claim_stopped`.
- U4: 1 private helper; 1 wiring generic over the pin kind.
- U5: `const fn` over `AltPin` + `Signal`.
- U6: shared SPI core; one `Info`.
- U7: `check_dma_buffer` generic over `&[T]`; the 3 unsafe blocks go.
- U8: home in `dma` and `wait`.
- U9: sealed `Peripheral { NAME, ENABLE }` supertrait + one call (with D1).
- U10: with D13.
- U11: move to `hil/common`.
