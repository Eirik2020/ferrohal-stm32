# Reference implementations

The architecture map's section 14, dissolved 2026-11-10. Reviewed at the commits
listed. Licences: stm32f4xx-hal and stm32h7xx-hal are 0BSD; embassy-stm32,
stm32g4xx-hal and imxrt are MIT or Apache-2.0.

| Reference | Use it for | Do not copy |
|---|---|---|
| embassy-stm32 (`fc3a052`) | Structure by IP version, generating `Peripherals` and routing traits from metadata, erased `Info`, clock config per version | Owning interrupts and NVIC priorities, panic on DMA error, global critical sections, unbounded `Drop`. It now pins git stm32-data metadata, so ported code may rely on metadata missing from crates.io metapac 21 |
| stm32f4xx-hal (`0eb673d`) | Type protection (`DMASet`, pin typestates), every DMA flag exposed, `next_transfer`, `&mut RCC` enable, an RTIC 2 monotonic that takes the timer by value | Unbounded spins in `stream_disable` and `wait()` |
| stm32h7xx-hal (`bcb9248`) | Capability-split DMA traits (`Stream`, `DoubleBufferedStream`, `MasterStream`) as the model for adapter traits | Critical sections around RCC |
