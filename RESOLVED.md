# FerroHAL: resolved debt

History: items moved out of [DEBT.md](DEBT.md) once done. Limits a fix left behind stay in DEBT.md as open items. Newest first within each section. Section numbers (§) refer to `ferrohal-architecture.md`.

## Crate-wide

- **No reachable panic, proven per chip** (§5, §13). `checks/no-panic` calls every public UART, DMA, GPIO and RCC operation with optimiser-opaque inputs under a panic handler that calls an undefined symbol, so the release build links only if no panic path survives. It found five metapac bounds asserts (fixed by masking stream indices) and two divisions in the H7 clock check (now `checked_div`). It covers all four chips.
- **Lap detection counts pending flags** (`write_count`). `read` adds the receive stream's unserved half and complete flags to the interrupt's count, so the interrupt may be almost a full lap late, not half. The HIL check "receive interrupt late by most of a lap" fails without the fix and passes with it, on both benches.
- **Errors keep their place in the stream.** The interrupt records each line error and idle line as a mark at its ring position; `try_read` returns the bytes before an error first and the error next; `read_chunk` ends the struck chunk with `ChunkEnd::Gap`.
- **Waits last the time asked, given a caller clock.** With a `CycleClock` (e.g. DWT), `read_within` lasts 1.2 times the 8N1 frame time on every board.
- **FerroWasp's transfer model, natively** (§11, §15). Receive: the ring never stops; `read_chunk` gives idle-ended, buffer-full and gap-ended chunks with a generation counter and `Health`. Transmit: `try_send` queues whole frames (up to 8, contiguous); the TX DMA interrupt chains them, or polling does. `TxHealth` counts frames, DMA errors and timeouts. The task/`UartIrq` protocol (`servicing`, `deferred`) was found on the bench to need fixing twice (an interrupt storm, then a CR read-modify-write race) and was.
- **Receive-only UARTs and inversion.** `UartRx::new` opens a UART with no TX pin or stream. RX/TX inversion are `Config` fields; `usart_v4` sets RXINV/TXINV, `usart_v2` refuses them. H753ZI run E receives the Lucid's SBUS case from inverted bit-banged frames; F401 run D receives plain SBUS.
- **Interrupt requirements are checked** (§7 hook 1). `UartIrq::interrupts()` names the vectors; `verify()` reads the NVIC and reports a disabled vector or mismatched priorities. Every bench firmware checks it, and that a masked receive vector is caught.
- **Resource descriptors.** `Wiring::RESOURCES` and `RxWiring::RESOURCES` list the USART, pins (port, number, AF), DMA streams (controller, stream, request) and vectors as `const` data, for comparison with a board manifest. Each chip's host test checks one wiring against FerroWasp's manifest where there is one.
- **Chip glue holds facts only.** Shared shapes (tokens, `Peripherals`, pin rows, stream vectors) are macros in `src/chips/shared/macros.rs`; what the RM0433 chips share (clock limits, DMA-reachable RAM, MPU walk) is `src/chips/shared/h7.rs`.
- **Routing tables are checked** against metapac by `tools/check_glue.py` (planted errors in AF, stream, RCC bit, DMAMUX request, extra pin or DMAMUX base all fail it).
- **The crate is in git** (its own repository, `ferrohal-stm32/`).
- **SPI master over DMA** (`spi_v2`, `spi_v4`, one engine). `start`/`on_interrupt`/`poll`/`received`/`abort`, a bounded blocking `transfer` and `embedded_hal::spi::SpiBus`; the receive stream's vector only. On hardware: every SPI pin entry and SPI DMA route on the F401, SPI1 to SPI5 on the H753ZI's Zio pins, each with loopback by interrupt and by polling, `SpiBus`, refusals, all four modes with SCK idling at CPOL, 8 SCK edges per byte counted by EXTI on the SCK pin, transfer time against the reported SCK, and abort then a clean transfer.
  - Found on the bench: `spi_v4` set MASTER with SSI still 0, so a mode fault cleared MASTER and no H7 transfer ran; SSI is now set first. `spi_v2` (SPE stays on) left the frames in flight at an abort to land in DR, so the next transfer received a stale byte first; `reset_state` now waits for the bus to go idle before draining DR.
  - Errata applied: ES0299 2.11.1 / ES0182 2.12.1 (SPE stays on, F4); ES0392 2.22.2 (SPE per transfer), 2.22.5 (CRCSIZE 7), 2.22.6 (one SCK held after EOT) on the H7.
- **EXTI** (`exti_v1`, `exti_h7`, one source): typed line tokens, `Ctrl` owning the shared registers, routing without taking the pin, a `Copy` handle for handlers. On hardware: rising, falling and both edges, masked lines, software trigger and level reads on both boards. A masked line doesn't latch its edges (RM0368 10.2.4, RM0433 20.3.1; the first doc said otherwise and the F401 corrected it).
- **Shared types:** `resource::{Resource, Signal, IrqError}` for every driver (`Resource::Uart` became `Resource::Peripheral`), `time::CycleClock`, and the bounded waits in `wait`.

## STM32F401RE

- **HSE in bypass runs on hardware.** Run A freezes from the ST-LINK's 8 MHz MCO (UM1724 7.9.1, MB1136 C-02 and later) to the same 84 MHz tree; USART2's host echo confirms it against the host's clock.
- **Clock limits per chip:** each F4 chip file has its own `CLOCK_LIMITS`.
- **Every route runs on hardware:** all three USARTs on every TX/RX pin and DMA stream RM0368 lists.
- **Errata review** (ES0299 Rev 6, all revisions): 2.2.7 RCC enable delay handled (`Rcc::enable` reads back); 2.2.11, 2.10.1 and 2.10.2 tracked in DEBT.md.
- **Docs:** RM0368, DS10086, ES0299 and UM1724 in `docs/`; RM0368 tables 28 and 29 match the glue. ES0222 is for the F401xB/C and doesn't apply.

## STM32F405RG

- **Values checked against the docs** (RM0090 Rev 22, DS8626 Rev 12, ES0182 Rev 19 in `docs/stm32f405/`):
  - DS8626: HCLK 168 MHz (VOS 1; 144 MHz with VOS 0), PCLK1 42, PCLK2 84; HSE crystal 4–26 MHz, external 1–50 MHz; PLL input 0.95–2.10 MHz; VCO 100–432 MHz.
  - RM0090: PLLM 2–63, PLLN 50–432; wait states (table 11) 30 MHz per WS at 2.7–3.6 V, 5 WS for 150–168 MHz; VOS is PWR_CR bit 14, reset value 1 (scale 1); SRAM1 + SRAM2 at 0x2000_0000 for all AHB masters, CCM at 0x1000_0000 CPU-only; 0.5 and 1.5 stop bits unavailable on UART4/5 (30.6.5).
- **UART4/5 reserved bit.** SR bit 9 (CTS) is reserved on UART4/5 (RM0090 30.6.1); `clear_tc` wrote 1 there and now writes 0, which is also right on the USARTs since CTS is only set with CTSE on.
- **Errata review** (ES0182): 2.2.13 RCC enable delay handled; 2.2.1 (prefetch, rev A), 2.2.19 (DMA2 AHB/APB2), 2.11.1 (IDLE), 2.11.2 (PE) tracked in DEBT.md. Not applicable: 2.2.11 (APB slowed during DMA: clocks are set once, before any driver), 2.6.1 (DAC), 2.11.3–2.11.8 (mute mode, CTS, smartcard, NACK, guard time, CTS with break: none used).
- **UART4 and UART5.** metapac 21 types them as `usart::Usart`; 0.5 and 1.5 stop bits are refused per instance (`Instance::FRACTIONAL_STOP_BITS`) by `Uart::new`, `UartRx::new`, `set_config` and `Uart::check`.
- **CCM RAM is refused for DMA:** `check_dma_buffer` accepts SRAM1 and SRAM2 only (0x2000_0000, 128 KiB).

## STM32H753ZI

- **HSE in bypass runs on hardware.** Run A, the Lucid mirror, freezes from the ST-LINK's 8 MHz MCO (UM2407 7.5.1): 8 MHz reference, 800 MHz wide VCO, 400 MHz core.
- **The D-cache can be on.** With it on, `check_dma_buffer` requires each buffer to lie wholly in one enabled MPU region with non-cacheable attributes (TEX, C, B; subregions honoured) that no higher region overlaps. The HIL turns both caches on with buffers in a non-cacheable `DMA_NC` block, and checks that cached RAM and the DTCM are refused.
- **Errata covered:** ES0392 2.20.4 (USART DMA lock-up; TRBUFF set on every UART transfer), 2.5.4 (DMAMUX routing; SE stays 0, SPOL 00).

## STM32H743VI

- **Values checked against DS12110 Rev 11** (`docs/stm32h743/`): HSE crystal 4–48 MHz, external 4–50 MHz; wide VCO 192–836 MHz (rev Y, table 40; 960 MHz rev V, table 137), medium 150–420 MHz; PLL input 2–16 MHz wide, 1–2 MHz medium; 400/300/200 MHz in scales 1/2/3. All match `src/chips/shared/h7.rs`.
