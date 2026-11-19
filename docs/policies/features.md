# Features policy

Normative. Goals G1, G2, G5 (`requirements/goals.md`). Record: ADR-007,
[`../decisions/2026-11-13-adapters-behind-features.md`](../decisions/2026-11-13-adapters-behind-features.md).

## Rules

- `default = []`. The default build is the guarantee: no time crate, no reachable panic,
  no public `unsafe fn` beyond G5's gated exception.
- Every optional feature is listed below: what it adds, whether it can panic, why.
  Added in the same commit as the feature.
- The core never calls an adapter. Adapter code sits behind its feature's `cfg`.
- Time adapter: converts to, or implements a time crate's trait over, FerroHAL's own
  types and timers. The core keeps `time::Hertz` and counted cycles (G1).
- A feature that can panic is a G2 exception: off by default, listed, reason stated, the
  owner's approval. Everything else stays in the no-panic link (`checks/no-panic`).
- Chip and IP-version features are not options (`docs/design/naming.md`).

## Optional features (2026-11-13)

| feature | adds | can panic | note |
|---|---|---|---|
| `rt` | cortex-m-rt vector table, `set-vtor`, the `device` module | no | every firmware needs it |
| `rtic` | `Peripherals::steal`, for RTIC's `peripherals = true` | no | G5 exception 1 (`safety.md`) |
| `defmt` | `defmt::Format` on public types | no | |
| `fugit` | `From` both ways between `time::Hertz` and fugit 0.6's `HertzU32` | no | in the no-panic link |
| `fault-injection` | `dma::fault`: marks a DMA stream to ignore stop requests | no | benches only (UARTDMA-009); a stuck mark is the only behaviour change, and nothing in FerroHAL sets one |
| `usb` | a `usb-device` bus over `synopsys-usb-otg` (0.4.0 from a fork pinned by revision, for ES0392 2.25.7, no panics and bounded waits), and `UsbVector`'s deferred service | no | in the no-panic link: every `UsbBus` trait method and the vector's service. `usb-device` itself is re-exported for the firmware's stack and classes, which are protocol code, the firmware's, outside G2 |

## Allowed, not built (owner 2026-11-13: not now)

- rtic-time: a FerroHAL timer as rtic-monotonics' monotonic (`docs/design/ferroforge-seam.md`, foot-gun 1).
- embassy-time: a time driver over a FerroHAL timer.

Roadmap: `docs/CHIPS.md`, Not supported yet.
