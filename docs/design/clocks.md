# Clocks

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 6, dissolved 2026-11-10.

- **[Decided]** The core is an explicit typed configuration per clock version, ported from embassy, and it is the only path that touches RCC. The user supplies the dividers; FerroHAL checks the limits and returns one frozen frequency record, which drivers must take (like stm32f4xx-hal's `Clocks`).
- **[Decided]** An optional solver may be added later. It would only output an explicit configuration and never configure hardware itself, and should ideally be computed at compile time so its result can be reviewed.
- **[Decided]** The prototype builds the explicit path only.
- Known trap *(measured)*: embassy-stm32's `rcc/f247.rs` writes `VOS Scale1` on every F4 with a PLL, which is reserved on the F401. Review each ported clock version against its reference manual.
