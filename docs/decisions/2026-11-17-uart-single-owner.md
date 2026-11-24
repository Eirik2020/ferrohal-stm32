# ADR-011: One owner per UART, and a scheduler-neutral `Service` trait (2026-11-17)

ADR-011 - Status: promoted

Promoted to: [uart-ownership.md](../design/uart-ownership.md).

Supersedes [ADR-004](archive/2026/2026-11-09-uart-capabilities.md) whole. Its premise,
one writer per register and no critical section in a driver, is kept.

## Context

The UART was the one driver handed out in pieces: an interrupt handle, a line handle and
separate transmit and receive halves. The pieces lived in different contexts, so their
shared state was atomics in a static, and the transmit DMA needed a protocol between
task and interrupt. That put a concurrency protocol inside a HAL whose execution model
gives execution to the scheduler, and a task could not name "the UART" as one type. SPI,
I2C, ADC and the timers already had one owner each. Interrupt service also differed per
driver: `on_interrupt` returned `()`, `bool` or an `Option`, and vectors came in four
shapes.

## Decision

1. One owner. `Uart<W>` holds the token, pins, DMA channels, buffers and all driver state
   as plain fields. Every operation takes `&mut self`. No handles, no `split` or `join`.
2. No atomics and no task-interrupt protocol in the driver.
3. Concurrency is the scheduler's: a task and a handler share the UART under its lock.
   The driver has no critical section.
4. `on_interrupt(&mut self) -> Events` serves the USART and its DMA, from any of its
   vectors or a task, and reports what happened. The handler acts on the
   events and reads nothing.
5. A read serves the receive channel's flags itself before counting, so reception does
   not depend on its vector.
6. A `Service` trait at the crate root,
   `trait Service { type Event; fn on_interrupt(&mut self) -> Self::Event; }`, is
   implemented by every interrupt-served driver. The `poll` aliases go.
7. One vector type, `resource::Vectors`, from `vectors()` on every such driver.
8. Half-duplex and TX/RX swap are `Config` fields. A receive-only UART is the same
   `Uart<W>`, opened by `Uart::new_rx`; transmit operations exist only on a full wiring.
9. `embedded-io`'s `write_all` and `read_exact` are overridden so they cannot panic.
10. USART v3 is served by the `usart_v4` code with only the PRESC write gated.

## Rejected

- Keeping the split for a reader and a writer in different tasks: the scheduler's lock
  already gives that, and the atomics were the price of avoiding it.
- Masking the USART's own enables while a task holds the UART: a second protocol, and
  the handler's priority is the scheduler's to manage.
- A crate-internal DMA-engine trait now: one engine exists, so a guess about the second
  would shape it.

## Consequences

- Firmware holds one shared resource per UART and locks it where it used a half.
- A low-priority lock holder delays the UART's handlers by its critical section; the
  bench measures that delay.
- UART-013 names the UART, not a transmit half.
- Names, files and per-driver detail: the design document.
