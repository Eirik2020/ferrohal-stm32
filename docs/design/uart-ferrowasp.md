# UART design for FerroWasp

Source: ADR-005, [`../decisions/2026-10-31-ferrowasp-integration.md`](../decisions/2026-10-31-ferrowasp-integration.md).


The architecture map's section 15's UART proposals, dissolved 2026-11-10;
realized in the shipped UART engine. The record is
[ADR-005](../decisions/2026-10-31-ferrowasp-integration.md).

- **[Proposal]** Receive never stops the DMA: the circular ring stays, and the interrupt records chunk boundaries (idle line, or the receiver timeout on `usart_v4`) and line errors at their ring positions. A reader takes chunks out of the ring by copy.
- **[Proposal]** Data loss is part of the stream: every overrun, DMA error, line error or restart ends a chunk with a gap and advances a generation counter, so a reader cannot miss a discontinuity. A `Health` snapshot counts each cause.
- **[Proposal]** Transmit is frame-atomic and queued: a frame goes in whole or not at all, and the transmit DMA interrupt chains the next one.
- **[Proposal]** Receive-only UARTs return the same receive type as full ones; `usart_v4` exposes RX and TX inversion.
- **[Proposal]** A UART configuration can be checked at compile time like the clock tree; a driver reports the interrupt vectors it needs and can verify their NVIC state read-only; `Wiring` exports its resources as data for board manifests; long waits can use a caller-supplied clock instead of counted passes.
