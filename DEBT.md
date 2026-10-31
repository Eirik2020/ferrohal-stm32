# FerroHAL: known debt and issues to fix

Status as of 2026-10-31: UART over DMA runs on two chips on hardware, and builds for two more.

- **F401:** `hil/run-f401.sh` passes 80 of 80 checks across four runs. Run A takes its clocks from HSE.
- **H753ZI:** `hil/run-h753.sh` passes 255 of 255 checks across five runs, with the caches off and on. Run A takes its clocks from HSE.
- **F405RG and H743VI:** the glue for FerroWasp's two flight controllers. Not yet run on hardware (see sections 3 and 5).
- **All four chips:** `checks/run.sh` (clippy, no reachable panic, glue against metapac, host tests) passes.

| Chip | Board | IP versions | Hardware-tested |
|---|---|---|---|
| STM32F401RE | Nucleo-F401RE | `usart_v2`, `dma_v2`, `gpio_v2`, `rcc_f4` | All three USARTs, all 10 TX/RX pin entries, all 9 DMA routes (RM0368 tables 28 and 29). USART2 runs through a host echo on the ST-LINK virtual COM port. |
| STM32F405RG | Foxeer F405 V2 | as the F401 | Not yet. Builds; glue checked against metapac. |
| STM32H753ZI | Nucleo-H753ZI | `usart_v4`, `dma_v1` + DMAMUX1, `gpio_v2`, `rcc_h7rm0433` | 7 of 8 U(S)ARTs, 24 of 46 TX/RX pin entries (Zio headers only), all 16 DMA1/DMA2 streams. Run A mirrors the TBS Lucid H7's UARTs at its rates (CRSF 420000, SBUS 100000 8E2, MSP 115200) on its DMA streams and clock tree, from HSE. |
| STM32H743VI | TBS Lucid H7 | as the H753ZI | Not as its own build. The H753ZI is the same die with crypto added, so the H753ZI bench covers the shared code; the H743VI glue differs only in its package's pins and peripherals. |

Severity:

- **High** can give wrong behaviour, or blocks a planned use (FerroWasp, F405).
- **Medium** is a gap someone will hit soon.
- **Low** is cleanup, or a limit worth knowing about.

Section numbers (§) refer to `ferrohal-architecture.md`.

---

## 1. Crate-wide (every chip)

### Correctness and robustness

- **Done: no reachable panic, proven per chip** (§5, §13).
  - `checks/no-panic` calls every public UART, DMA, GPIO and RCC operation with optimiser-opaque inputs, under a panic handler that calls an undefined symbol, so the release build links only if no panic path survives. It found five metapac bounds asserts (fixed by masking stream indices) and two divisions in the H7 clock check (now `checked_div`).
  - **Limit:** it proves the paths it exercises. A new public function must be added to it.
- **Done: lap detection counts pending flags** ([uart/stream_dma.rs](src/uart/stream_dma.rs), `write_count`).
  - `read` adds the receive stream's unserved half and complete flags to the interrupt's count, so the interrupt may now be almost a full lap late, not half. The HIL check "receive interrupt late by most of a lap" masks the vector across both marks: it fails with a false `BufferOverrun` without the fix and passes with it, on both chips.
  - **Remaining limits, documented on `Buffers::rx`:** the interrupt must be served within one ring's worth of bytes, and the reading code must never preempt `UartIrq`.
- **Medium: the v2 error clear can take a good byte** ([usart_v2](src/usart_v2/mod.rs), `Line::take_errors`).
  - **Issue:** clearing errors needs an SR read followed by a DR read. If DMA hasn't yet taken the byte in DR, the DR read consumes it.
  - **Fix:** none; it's inherent to v2. Document it as "one byte may be lost per reported line error". v4 clears through ICR and is unaffected.
- **Medium: the v2 parity-error clear sequence isn't followed.**
  - **Issue:** RM0368 says to wait for RXNE before clearing PE. The driver clears PE at once.
  - **Fix:** follow the manual's sequence, or confirm on hardware that clearing early is harmless.
- **Low: transmit DMA errors are reported after the frame was queued.**
  - **Issue:** `try_send` returns once a frame is queued; a DMA error on it shows in `TxHealth`, and as an error from the next `write` or `flush`.
- **Done: errors keep their place in the stream.**
  - The interrupt records each line error, and each idle line, as a mark at its ring position. `try_read` returns the bytes before an error first, and the error on the next call. `read_chunk` ends the chunk the error struck with `ChunkEnd::Gap`.
  - **Limit:** a line error's position is where DMA had written to when the interrupt ran, so the damaged byte is the chunk's last or near it, not identified exactly. A full mark queue (16 marks between reads) reports the error without a position.
- **Done: waits last the time asked, given a caller clock.**
  - With a `CycleClock` set (`Uart::set_clock`, e.g. the DWT counter), every wait is measured: `read_within` lasts 1.2 times the 8N1 frame time on every board, the 1.2 being the 12-bit frame the HAL budgets for.
  - Without one, waits count passes against a floor of half the fastest pass measured (`RX_PASS_CYCLES` 224, `TX_PASS_CYCLES` 48). They are never short, but run 2.5 (H753 cached), 3.6 (F401) and 7.0 (H753 uncached) times long.
  - **Limit:** the floors are measurements. The HIL check "counted and clocked waits within their bounds" fails if a pass gets faster than its floor, a counted wait exceeds 12 times its time, or a clocked wait exceeds 1.5 times. Every new chip must pass it.
- **Low: a corner case can fail to keep NDTR and the half-lap count in step.**
  - **Issue:** if interrupts keep landing between the reads, `write_count` gives up after `COUNT_TRIES` and uses the last sample.
  - **Fix:** this is theoretical at UART rates. Count how often it happens, if anyone wants a number.

### API gaps

- **Done: FerroWasp's transfer model, natively** (§11, §15).
  - **Receive:** the ring never stops; `read_chunk` gives idle-ended, buffer-full and gap-ended chunks, with a generation counter and a `Health` snapshot. That is what FerroWasp's `UartRxDmaTransfer` needs, by copy.
  - **Transmit:** `try_send` queues whole frames (up to 8, contiguous in the transmit buffer); the transmit DMA interrupt chains them, or polling does when its vector is unbound. `TxHealth` counts frames sent, DMA errors and timeouts.
  - **Protocol:** the task and `UartIrq` share the transmit stream. Every write to it by the task happens with `servicing` set; an interrupt that finds it set only masks the stream and leaves the work to the task, which services again before it lets go. The first versions of this hung (an interrupt storm) and lost frames (a read-modify-write of CR racing a transfer start); both were found on the bench and fixed.
  - The compatibility adapter is built at swap time, as a `TRANSITION:` item (§15).
- **Medium: `Uart::new` drops its tokens on error.**
  - **Issue:** if `new` fails, the pin, stream and USART tokens are consumed.
  - **Mitigated:** a frame can now be checked while compiling (`Uart::check::<USART6>(&CLOCKS, &config)` in a `const` assert, as both bench boards do), so the configuration errors need never reach `new`. What remains at run time is buffers and a stream left running.
  - **Fix:** return the tokens with the error.
- **Medium: there's no `release()`/`free()`.**
  - **Issue:** once `Uart::new` has the pins and streams it never gives them back, so there's no way to tear the driver down.
- **Medium: the blocking `embedded_io::Read::read` has no bound.**
  - **Issue:** the trait requires it to block until data arrives. `read_within(buf, frames)` is the bounded alternative.
  - **Fix:** document `read_within` as the form to use in FerroForge tasks.
- **Done: receive-only UARTs and inversion.**
  - `UartRx::new` opens a UART with no TX pin or stream, the transmitter off; its `UartIrq` has no transmit part. RX and TX inversion are `Config` fields; `usart_v4` sets RXINV and TXINV, `usart_v2` refuses them with `ConfigError::Unsupported`.
  - On the H753ZI, run E receives the TBS Lucid H7's SBUS case (receive-only USART1, 100000 8E2, inverted, its DMA stream) from inverted frames bit-banged on the link's other end, and the full suite checks inverted loops on every UART. On the F401, run D receives plain SBUS frames, and the refusal is checked while compiling.
- **Medium: the frame format is limited.**
  - **Issue:** data is always 8 bits, with optional parity as a ninth. There's no 7- or 9-bit data, no RTS/CTS, no half-duplex, no RS-485 driver enable and no LIN.
- **Done, in part: interrupt requirements are checked** (§7 hook 1, §15).
  - `UartIrq::interrupts()` names the vectors a UART needs (line, receive stream, and transmit stream unless receive-only). `verify()` reads the NVIC, without writing to it, and reports a vector left disabled or at a priority different from the others. Every bench firmware checks it for each UART, along with the converse: with the receive vector masked, `verify()` must name it.
  - **Remaining:** `verify()` can't know the priorities of the tasks that use the UART, so "above every task that uses it" is still the application's job.
- **Done: resource descriptors.**
  - `Wiring::RESOURCES` and `RxWiring::RESOURCES` list, as `const` data, the USART, each pin with its port, number and alternate function, each DMA stream with its controller, number and request, and each interrupt vector. A board manifest (such as FerroWasp's) can be compared against them while compiling or in a host test; each chip's host test `wiring_describes_itself` checks one wiring's list.
- **Medium: interrupts can't be masked from the handle** (§7 hook 1).
  - **Issue:** `UartIrq` reads and clears flags but can't turn the peripheral's interrupt-enable bits on or off.
- **Medium: there's no timer handover** (§7 hook 5, §8 foot-gun 1).
  - **Issue:** there's no timer driver, and no path for rtic-monotonics 3 to consume a FerroHAL timer token.
- **Low: `set_config` exists only on the whole `Uart`.**
  - **Issue:** after `split()` the frame can't be changed, and a byte arriving during the change is lost. `Uart::halves()` lends both halves without giving up `set_config`.
- **Low: parts of the DMA API are untested.**
  - **Issue:** double buffering (`Transfer::double_buffer`, `set_idle_buffer`, `current_target`), FIFO threshold mode and the raw `Channel::start` used directly by a caller have never run on hardware.

### Architecture and process

- **Mitigated: routing tables are hand-written, and now checked** (§5, §10).
  - `tools/check_glue.py` compares every routed U(S)ART's pins and alternate functions, RCC enable bit, and DMA routes or DMAMUX request IDs, plus every GPIO port and DMA controller enable bit and the DMAMUX bases, with the pinned metapac's metadata: 17 facts on the F401, 29 on the F405, 39 on each H7. Planting a wrong alternate function, stream, RCC bit, DMAMUX request, extra pin or DMAMUX base fails it.
  - **Still open:** the generator itself. The check reads the glue's macro rows as text, so their layout must stay as it is.
- **Medium: two source files each serve two IP versions** (§2, §9).
  - **Issue:** [dma_stream.rs](src/dma_stream.rs) serves both `dma_v1` and `dma_v2`, and [uart/stream_dma.rs](src/uart/stream_dma.rs) serves both `usart_v2` and `usart_v4`. That removes duplicate code, but it conflicts with "one self-contained module per IP version, with a stable hash".
  - **Fix:** decide whether a module's hash covers its shared sources, or split the files once they diverge.
- **Done: chip glue holds facts only.** The shapes every chip shares (tokens, `Peripherals`, pin rows, stream vectors) are macros in `src/chips/shared/macros.rs`. What the RM0433 chips share beyond their IP versions (clock limits, DMA-reachable RAM, the MPU walk) is `src/chips/shared/h7.rs`. A chip file is its rows and its memory map.
- **Medium: the feature hierarchy departs from §2.**
  - **Issue:** each chip feature turns on its family marker, rather than the family bundling its chips. Two chip features together are caught only by `compile_error!`, which lists every pair and has to be updated for every new chip (six pairs at four chips).
- **Medium: requirements tooling is missing** (§9).
  - **Issue:** there are no `requirements.yaml` files, no schema, no requirement IDs in module docs and no publish tool.
- **Low: kernel clock equals bus clock.**
  - **Issue:** `Uart::new` uses the bus clock as the USART kernel clock. That holds on F4, and on H7 only because `freeze` forces the kernel clock mux to the bus clock.
  - **Fix:** add a kernel-clock field to `Info` before offering other muxes.
- **Low: the crate isn't in git.**
  - **Issue:** it isn't a repository yet, and the architecture doc says it should get its own (§1).

### GPIO

- **Medium: `split()` doesn't know the package.**
  - **Issue:** it hands out all 16 pins on every port the register map has, whether or not the package bonds them out.
  - **Fix:** add a per-package pin list to the glue.
- **Low: missing GPIO features.**
  - **Issue:** there's no open-drain mode, no EXTI and no runtime mode switching (`with_*` style temporary modes).

### Test harness (`hil/`)

- **Medium: some error paths are never exercised.**
  - **Issue:** there are no checks for DMA transfer errors (the metapac spike's `inject-dma-error` wasn't ported), USART-level overrun, noise or a stop timeout.
  - **Covered so far:** ring overrun, a late receive interrupt and break on every jumpered link. On the F401's cross-wired run, a baud mismatch must be reported as a framing error, and a parity mismatch as a parity error.
- **Medium: the H753ZI bench reaches the Zio headers only.**
  - **Issue:** this board has no morpho headers (CN11, CN12) fitted, so `hil/run-h753.sh` covers 7 of 8 U(S)ARTs and 24 of 46 pin entries. UART8 can't be looped at all: its only TX pin, PE1, is on the morpho headers only. The Lucid's USART1 RX pin, PA10, and PA8, PA9, PB14 and PG9 are morpho-only too. UART5's PB13 reaches the Zio header only through SB7, which isn't fitted.
  - **Fix:** fit the morpho headers (the 38-of-46 plan, UART8 included), and SB7 for PB13. PA1, PA2, PA11 and PA12 need board rework (Ethernet, USB).
- **Low: USART2 on the F401 is checked through the ST-LINK.**
  - **Issue:** break, baud and parity changes don't survive the virtual COM port bridge, so USART2 runs echo, stream and ring overrun only. Solder bridges SB13/SB14 off and SB62/SB63 on would allow a direct link.
- **Low: probes are picked by VID:PID.**
  - **Issue:** `0483:374b` and `0483:374e` work only while there's one probe of each type. Switch to serial numbers for a shared bench.
- **Low: clock checks are indirect.**
  - **Issue:** a clock tree counts as verified when `freeze`'s record matches the `const` one and the baud rates work. Nothing measures it, for example with MCO on a scope.

---

## 2. STM32F401RE (Nucleo-F401RE)

### Clocks (`rcc_f4`)

- **Done: limits per chip.** Each F4 chip file has its own `CLOCK_LIMITS` (the F401's 84/42/84 MHz, VCO 192–432 MHz and 2 wait states; the F405's in section 3).
- **Done: HSE in bypass runs on hardware.** Run A freezes from the ST-LINK's 8 MHz MCO (UM1724 7.9.1, boards MB1136 C-02 and later) to the same 84 MHz tree, and USART2's host echo confirms the result against the host's clock. HSE with a crystal (oscillator mode) waits for the Foxeer.
- **Low: the regulator scale relies on the reset value.** VOS isn't written; the F401 reaches 84 MHz in its reset scale (scale 2). See section 3 for the F405.
- **Low: the USB clock isn't checked.**
  - **Issue:** `pll_q` is reported but not required to be 48 MHz.
- **Low: only one supply range.**
  - **Issue:** `Supply` offers 2.7–3.6 V only.

### USART (`usart_v2`)

- **Done: UART4 and UART5 (F405).** metapac 21 types them as `usart::Usart`, the same as the USARTs, and the driver writes no bit they lack. What they lack that a frame can ask for, 0.5 and 1.5 stop bits, is refused per instance (`Instance::FRACTIONAL_STOP_BITS`) by `Uart::new`, `UartRx::new`, `set_config` and `Uart::check`.
- **Medium: parity errors can be lost while transmitting** (ES0299 2.10.2).
  - **Issue:** in full duplex, reading SR and then writing DR clears PE. The TX DMA writes DR, so a USART that is transmitting while it receives a parity error can lose the PE flag before `UartIrq` runs.
  - **Fix:** none in hardware. Document that parity detection on `usart_v2` is reliable only when the receiving USART isn't transmitting at the same time.
- **Done: every route runs on hardware.** `hil/run-f401.sh` covers all three USARTs on every TX/RX pin and DMA stream RM0368 lists for them.

### DMA (`dma_v2`)

- **Errata review: done** (ES0299 Rev 6, which applies to all silicon revisions A, 1 and Z):
  - **2.2.7, delay after enabling an RCC clock:** handled. `Rcc::enable` reads the register back.
  - **2.2.11, DMA2 corruption with concurrent AHB and APB2 requests:** doesn't affect the UART, because USART1/6 are on APB2 and their buffers are in SRAM. It does hit DMA2 writes to GPIO, which is how DShot works. Before any GPIO-DMA driver, route those writes through DMA2's memory port as the erratum describes.
  - **Medium, 2.10.2: PE cleared by a write to DR** (see USART below).
  - **Medium, 2.10.1: IDLE isn't detected when the sender's clock runs faster than the receiver's.** No workaround exists. This hits the planned idle-line chunk RX for FerroWasp, which will need a timeout (a timer or RTOR-like fallback) and not IDLE alone.
- **Low: FIFO errors are advisory.**
  - **Issue:** a FIFO error on the TX stream is cleared and ignored, matching FerroWasp's policy. On the RX ring it's reported as `DmaOverrun`. Confirm both on hardware.

### GPIO and package

- **Low: phantom pins.**
  - **Issue:** the LQFP64 package has only PD2, PH0 and PH1 outside ports A to C, and no port E, but `split()` hands them all out.

### Docs for this chip

- **Done:** RM0368, DS10086, ES0299 and UM1724 are in `docs/` (workspace root).
  - RM0368's DMA request tables (28 and 29) match the glue.
  - UM1724's connector and solder-bridge tables confirm the pins.
  - ES0222, also in `docs/`, is for the F401xB/C and doesn't apply to this chip.

---

## 3. STM32F405RG (Foxeer F405 V2)

The glue (`src/chips/stm32f405rg.rs`) routes all six U(S)ARTs, their 12 LQFP64 pin entries and their 12 DMA routes, checked against metapac. Its host tests check the Foxeer's USART2 resources against its manifest (DMA1 streams 5 and 6, channel 4), the Foxeer's 168 MHz tree from its 8 MHz crystal, and the CCM refusal below.

- **High: not run on hardware.**
  - **Issue:** there's no Nucleo for it, so the first hardware run is on the Foxeer F405 V2 itself, which needs the owner's go-ahead and a loopback plan for its pads.
- **High: the docs aren't in the repo.**
  - **Issue:** the clock limits (168/42/84 MHz, HSE 4–26 MHz crystal or 1–50 MHz bypass, PLL input 0.95–2.1 MHz, VCO 100–432 MHz, 30 MHz per wait state up to 5), the memory map and the UART4/5 stop-bit limit are from memory of DS8626 and RM0090, cross-checked with metapac only where metapac has the fact (routing, RCC bits).
  - **Fix:** add RM0090, DS8626 and ES0182 to `docs/stm32f405/` and check each value, then do the errata review.
- **Medium: VOS relies on its reset value.**
  - **Issue:** 168 MHz needs scale 1, which is the F405's reset value (PWR_CR VOS = 1), so `freeze` doesn't write it. A bootloader that left scale 2 would make 168 MHz out of spec.
  - **Fix:** once RM0090 is in the repo, write the scale in `freeze` per chip (the F405's VOS is one bit, the F401's two, so the value comes from the chip file).
- **Done: CCM RAM is refused for DMA.** `check_dma_buffer` accepts SRAM1 and SRAM2 only (0x2000_0000, 128 KiB); a buffer in the 64 KiB CCM RAM, which the DMA can't reach, fails with `BufferUnreachable`.
- **Low: phantom pins.** As on the F401: ports E to I are handed out though LQFP64 bonds only ports A to C, PD2, PH0 and PH1.

---

## 4. STM32H753ZI (Nucleo-H753ZI)

### Memory and cache

- **Done: the D-cache can be on.**
  - With it on, `check_dma_buffer` (now public, and run by `Uart::new`) requires each buffer to lie wholly in one enabled MPU region with non-cacheable attributes (TEX, C and B; subregions honoured) that no higher-numbered region overlaps. Otherwise it returns `BufferCacheable`.
  - The H7 HIL turns both caches on, puts the buffers in a 16 KiB block marked non-cacheable by MPU region 0 (`DMA_NC` in memory.x), and passes every check. A buffer in cached RAM and one in the DTCM are both refused.
  - **Limits:** FerroHAL reads the MPU (stepping RNR, then restoring it), so the firmware must not reprogram the MPU while a `Uart` is being opened. There's no cache-maintenance mode for buffers in cacheable memory.
- **Medium: the DTCM goes unused.**
  - **Issue:** DMA1/DMA2 can't reach the DTCM, so the test firmware's `memory.x` puts everything in AXI SRAM, including the stack. `check_dma_buffer` enforces the DMA-reachable ranges.
  - **Fix:** a firmware that wants the DTCM for speed needs a linker section for DMA buffers.

### Clocks and power (`rcc_h7rm0433`)

- **Medium: no VOS0 (480 MHz).**
  - **Issue:** VOS0 exists only on silicon rev V, needs SYSCFG ODEN, and has its own flash table. The silicon revision isn't read (DBGMCU IDCODE).
- **Medium: limits are the lowest common denominator.**
  - **Issue:** the wide VCO is capped at 836 MHz (rev Y, DS12117 table 39), even though rev V allows 960 MHz (table 136). Embassy uses 960 MHz.
  - **Fix:** detect the revision if the headroom matters.
- **Medium: LDO only, and the supply write is once-only.**
  - **Issue:** only `Supply::Ldo` is offered (the H753 has no SMPS), and PWR_CR3 can be written once per power-on reset. If something has already locked another configuration, `freeze` returns `SupplyTimeout`.
- **Medium: much of the clock tree isn't offered.**
  - **Issue:** PLL2, PLL3, the fractional PLL mode, CSI, HSI48, HSIDIV other than 1, and TIMPRE.
- **Medium: kernel clock muxes are forced.**
  - **Issue:** `freeze` always sets USART16910SEL = PCLK2 and USART234578SEL = PCLK1, so no other kernel clock is possible.
- **Done: HSE in bypass runs on hardware.** Run A, the Lucid mirror, freezes from the ST-LINK's 8 MHz MCO (UM2407 7.5.1): 8 MHz reference, 800 MHz wide VCO, 400 MHz core, the same tree as `CLOCK_CONFIG`. The Lucid itself uses a crystal (oscillator mode), untested until it is on the bench.
- **Low: no IO compensation cell.**
  - **Issue:** SYSCFG CCCSR isn't enabled. It's needed for fast GPIO above about 50 MHz, not for a UART.
- **Low: wait-state tables are transcribed.**
  - **Issue:** the RM0433 table 17 values were checked once, against both embassy and the manual. They are a transcription, not generated.

### USART (`usart_v4`)

- **Done: RX inversion** (SBUS on the Lucid): see section 1.
- **Medium: no LPUART1.**
  - **Issue:** it uses the `Lpuart` register block (20-bit BRR), sits in the D3 domain, and its DMA comes through DMAMUX2 and the BDMA.
- **Low: hardware FIFOs unused.**
  - **Issue:** the RX/TX FIFOs (`FIFOEN`) are off. DMA serves one byte per request.
- **Low: most instances untested.**
  - **Issue:** USART1/3/6 and UART4/5/7/8 are routed but untested. USART3 is the ST-LINK VCP on PD8/PD9.

### DMA (`dma_v1`, DMAMUX1)

- **Medium: only DMA1 and DMA2 are supported.**
  - **Issue:** there's no BDMA (needed for D3 peripherals such as LPUART1 and SPI6) and no MDMA.
- **Low: DMAMUX overruns aren't watched.**
  - **Issue:** the overrun flags (CSR SOFx) aren't read. ES0392 2.5.1 says writing CFR doesn't assert them anyway.
- **Errata covered:**
  - ES0392 2.20.4 (a USART's DMA stream can lock up): TRBUFF is set on every UART transfer.
  - ES0392 2.5.4 (wrong DMAMUX routing): never triggered, because SE stays 0 and SPOL stays 00.

### GPIO and package

- **Low: phantom pins.**
  - **Issue:** ports I to K are handed out though LQFP144 doesn't bond them, and port H only has PH0 and PH1.

### Errata review

- **Medium: the review is partial.**
  - **Issue:** ES0392 was checked only for USART, DMA, DMAMUX and the flash table. Review the rest (cache, system, flash ECC) before relying on those areas.

---

## 5. STM32H743VI (TBS Lucid H7)

The glue (`src/chips/stm32h743vi.rs`) is the H753ZI's with the LQFP100 package's pins (USART6 on PC6/PC7 only; no port F or G pins) and its peripherals (no CRYP or HASH, no SPI5). The clock limits and DMA memory map are shared with the H753 in `src/chips/shared/h7.rs`. Its host test checks the Lucid's USART6 resources against its manifest (DMA1 streams 6 and 0, requests 72 and 71).

- **Medium: its own build isn't run on hardware.**
  - **Issue:** the H753ZI bench runs the same die and the same driver code, but under the H753ZI glue.
  - **Fix:** build run A with the H743VI feature and run it on the H753ZI Nucleo (every pin it uses is bonded on LQFP100), or on the Lucid itself.
- **Low: DS12110 isn't in the repo.** The H743's datasheet; its limits are taken to match DS12117's (same die). RM0433 and ES0392 already cover the H743.

---

## 6. Open questions carried from the architecture doc

- **Kill condition for the prototype** (§11): still undecided.
- **How to measure reuse** (§11): the shared UART engine now runs unchanged on `usart_v2` and `usart_v4`, which gives a data point. The per-IP `Line` is about 150 lines; the shared engine is about 800.

## 7. Transition items

Code that exists only for the move of FerroWasp onto FerroHAL (§15). Each is marked `TRANSITION:` in its documentation and is removed once FerroWasp uses FerroHAL's native model.

- None yet. The compatibility adapter for FerroWasp's `UartRxDmaTransfer` and `UartTxDmaTransfer` will be the first, built at swap time.

