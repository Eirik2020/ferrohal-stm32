# FerroHAL: open debt and issues

Open items only. Finished work moves to [RESOLVED.md](RESOLVED.md), which is history and needn't be read to work on the crate.

Status as of 2026-10-31:

- **F401:** `hil/run-f401.sh` passes 188 of 188 checks across ten runs: UART (A to D, A from HSE), SPI and EXTI (E to G), timers (H), compare lanes read back by a sampler (I), the watchdog across its reset (J).
- **H753ZI:** `hil/run-h753.sh` passes 411 of 411 checks across eleven runs: UART (A to E, caches off and on, A from HSE), SPI and EXTI (F to H), timers (I), the Lucid's DShot lanes and a complementary output read back by samplers (J), the watchdog across its reset (K).
- **F405RG, H743VI:** build, glue checked against metapac, values checked against their docs. Not yet run on hardware.
- **All four chips:** `checks/run.sh` (clippy, no reachable panic, glue against metapac, host tests) passes.

| Chip | Board | IP versions | Hardware-tested |
|---|---|---|---|
| STM32F401RE | Nucleo-F401RE | `usart_v2`, `spi_v2`, `exti_v1`, `dma_v2`, `gpio_v2`, `rcc_f4` | All three USARTs, all 10 TX/RX pin entries, all 9 USART DMA routes; USART2 through a host echo on the ST-LINK VCP. All three SPIs on all 15 pin entries and all 10 SPI DMA routes. EXTI on PA12 and on every SPI SCK pin. |
| STM32F405RG | Foxeer F405 V2 | as the F401 | Not yet. |
| STM32H753ZI | Nucleo-H753ZI | `usart_v4`, `spi_v4`, `exti_h7`, `dma_v1` + DMAMUX1, `gpio_v2`, `rcc_h7rm0433` | 7 of 8 U(S)ARTs, 24 of 46 TX/RX pin entries (Zio only), all 16 DMA1/DMA2 streams. Run A mirrors the TBS Lucid H7 (CRSF 420000, SBUS 100000 8E2 inverted, MSP 115200, its streams and clock tree). SPI1 to SPI5 on the Zio-reachable pins (SPI1 on the Lucid's pins and streams). EXTI on PD6 and on every SPI SCK pin. |
| STM32H743VI | TBS Lucid H7 | as the H753ZI | Not as its own build; same die as the H753ZI. |

Severity: **High** can give wrong behaviour or blocks a planned use (FerroWasp). **Medium** is a gap someone will hit soon. **Low** is cleanup, or a limit worth knowing. Section numbers (§) refer to `ferrohal-architecture.md`.

---

## 1. Crate-wide

### Correctness

- **Medium: the v2 error clear can take a good byte** ([usart_v2](src/usart_v2/mod.rs), `Line::take_errors`).
  - Clearing errors needs an SR read then a DR read; if DMA hasn't taken the byte in DR yet, the DR read consumes it. Inherent to v2 (v4 clears through ICR). Document "one byte may be lost per reported line error".
- **Medium: the v2 parity-error clear sequence isn't followed.**
  - RM0368 and RM0090 say to wait for RXNE before clearing PE; the driver clears at once. Follow the sequence, or confirm on hardware that it's harmless.
- **Medium: parity errors can be lost while transmitting** (`usart_v2`; ES0299 2.10.2, ES0182 2.11.2).
  - In full duplex, an SR read followed by a DR write clears PE, and the TX DMA writes DR. No hardware fix; document that v2 parity detection is reliable only while the receiving USART isn't transmitting.
- **Medium: IDLE can be missed** (`usart_v2`; ES0299 2.10.1, ES0182 2.11.1).
  - IDLE isn't detected when the sender's clock runs faster than the receiver's. No workaround; idle-ended `read_chunk` on v2 needs a timeout fallback (timer), not IDLE alone.
- **Low: transmit DMA errors are reported after the frame was queued.** A DMA error shows in `TxHealth` and as an error from the next `write` or `flush`.
- **Low: NDTR and the half-lap count can fall out of step** in a theoretical corner case: if interrupts keep landing between the reads, `write_count` gives up after `COUNT_TRIES` and uses the last sample.
- **Low: limits of the receive ring**, documented on `Buffers::rx`: the interrupt must be served within one ring's worth of bytes, and the reading code must never preempt `UartIrq`.
- **Low: line-error positions are approximate.** A mark sits where DMA had written to when the interrupt ran, so the damaged byte is the chunk's last or near it. A full mark queue (16 marks between reads) reports the error without a position.
- **Low: counted-wait floors are measurements** (`RX_PASS_CYCLES` 224, `TX_PASS_CYCLES` 48). Without a `CycleClock`, waits run 2.5–7 times long. The HIL check "counted and clocked waits within their bounds" must pass on every new chip.
- **Low: the no-panic check proves only the paths it calls.** A new public function must be added to `checks/no-panic`.
- **Low: SPI counted waits run long without a `CycleClock`.** `PASS_CYCLES` (112) is half the fastest polling pass measured (233 cycles, H753 with caches on, measured once by hand); the bench checks the floor on every SPI link, but its SPI runs keep the caches off, so the cached figure isn't rechecked. Waits run up to about 3 times long on the F401 and 6 on the uncached H753.

### API gaps

- **Medium: `Uart::new` drops its tokens on error.** Mitigated by checking frames while compiling (`Uart::check::<U>` in a `const` assert); buffers and a running stream remain run-time errors. Fix: return the tokens with the error.
- **Medium: there's no `release()`/`free()`;** the driver can't be torn down.
- **Medium: the blocking `embedded_io::Read::read` has no bound.** Document `read_within` as the form for FerroForge tasks.
- **Medium: the frame format is limited:** 8 data bits (parity as a ninth); no 7/9-bit data, RTS/CTS, half-duplex, RS-485 DE or LIN.
- **Medium: interrupts can't be masked from the handle** (§7 hook 1). `UartIrq` can't turn the peripheral's interrupt-enable bits on or off.
- **Medium: there's no timer handover to rtic-monotonics** (§7 hook 5, §8 foot-gun 1). FerroHAL's `Tick` and `Timebase` run timers, but rtic-monotonics 3 can't take a FerroHAL timer token as its monotonic.
- **Medium: timer features still missing:** input capture, one-pulse mode, starting several timers from one trigger (FerroWasp's lanes can start from one timer's trigger output; FerroHAL starts them one after another in software), dead time, the break input, and switching a lane's stream between transmit and sampling for bidirectional DShot (FerroWasp's reply window).
- **Medium: the watchdog has no window, and no freeze while debugging.** IWDG1's window register (H7) stays at its reset value (no window), and DBGMCU's watchdog freeze is never set, so a halted core lets a started watchdog reset the chip under the debugger.
- **Low: the watchdog timeout is nominal.** It's worked out from the LSI's typical 32 kHz; the real LSI spans 17 to 47 kHz on the F4 (DS10086, DS8626) and 29.4 to 33.6 kHz on the H7 (DS12110). The bench measured 53.3 ms on the F401 and 50.5 ms on the H753ZI for a nominal 50 ms. Measuring the LSI against a timer would make it exact.
- **Low: the watchdog bench run needs reset catch off.** probe-rs halts a core it sees reset, so `hil/run-*.sh` run the watchdog check with `--no-catch-reset`; the second boot waits before reporting so the host finds RTT again.
- **Low: arming and staging lanes takes time.** About 14 µs for two lanes on the F401 at 84 MHz, measured by the bench; a frame scheduler must allow for it.
- **Low: sampler rates.** The bench samples at about 12 times the DShot600 bit rate (7 MHz on the F401, 7.1 MHz on the H753) without losing samples; faster rates are untested.
- **Low: a tick or timebase rate must divide the timer clock exactly.** `Tick` and `Timebase` refuse a rate they would have to round (`ConfigError::Inexact`) rather than drift.
- **Low: some timers share their update vector** (the F4's `TIM1_UP_TIM10`, the H7's `TIM8_UP_TIM13`). `Tick::on_interrupt` returns `false` when the flag isn't its own, but the firmware must call every sharer's handler from the one vector.
- **Low: `verify()` can't check task priorities.** It checks the UART's vectors are enabled at one priority; "above every task that uses it" stays the application's job.
- **Low: `set_config` exists only on the whole `Uart`;** after `split()` the frame can't change. `Uart::halves()` lends both halves meanwhile.
- **Low: parts of the DMA API are untested:** double buffering (`Transfer::double_buffer`, `set_idle_buffer`, `current_target`), FIFO threshold mode, and `Channel::start` used directly.
- **Medium: the SPI is master, full duplex, 8-bit only.** No slave mode, simplex or half-duplex, 16-bit frames, hardware NSS or CRC.
- **Low: SPI bit order is unchecked on hardware.** A MOSI-to-MISO loop returns the bytes whichever bit goes first, so `BitOrder::LsbFirst` is only shown to transfer.
- **Low: EXTI covers GPIO lines 0 to 15 only,** as interrupts: no event mode (EMR), and none of the internal lines (PVD, RTC, USB wake-up and the like).
- **Low: EXTI owns the whole SYSCFG token.** It needs only EXTICR, but SYSCFG also holds the H7's I/O compensation cell and the F4's memory remap, which later drivers will want.
- **Low: FIFO errors are advisory.** On the TX stream they're cleared and ignored (FerroWasp's policy); on the RX ring they're `DmaOverrun`. Confirm both on hardware.

### Architecture and process

- **Medium: the routing generator doesn't exist** (§5, §10). Glue rows are hand-written; `tools/check_glue.py` checks them against metapac (U(S)ARTs, SPIs and timers: 82 facts on the F401, 128 on the F405, 129 on the H743, 132 on the H753) by reading the rows as text, so their layout must stay as it is. It doesn't check the SPI kernel-clock column or the EXTI vectors.
- **Medium: five source files each serve two IP versions** (§2, §9). [dma_stream.rs](src/dma_stream.rs) serves `dma_v1` and `dma_v2`, [exti.rs](src/exti.rs) `exti_v1` and `exti_h7`, [timer.rs](src/timer.rs) `timer_v1` and `timer_v3`; the engines [uart/stream_dma.rs](src/uart/stream_dma.rs) and [spi/stream_dma.rs](src/spi/stream_dma.rs) serve two IP versions each through their `Line`/`Unit`. Decide whether a module's hash covers shared sources, or split once they diverge.
- **Medium: the feature hierarchy departs from §2.** Chip features turn on their family marker, and two chip features together are caught only by a `compile_error!` listing every pair (six at four chips).
- **Medium: requirements tooling is missing** (§9): no `requirements.yaml`, schema, requirement IDs or publish tool.
- **Low: kernel clock equals bus clock.** True on F4; on H7 only because `freeze` forces the kernel muxes. Add a kernel-clock field to `Info` before offering other muxes.

### GPIO

- **Medium: `split()` doesn't know the package.** It hands out all 16 pins on every port the register map has, bonded or not (see each chip's package below). Fix: a per-package pin list in the glue.
- **Low: missing GPIO features:** open-drain, runtime mode switching.

### Test harness (`hil/`)

- **Medium: some error paths are never exercised:** DMA transfer errors (the spike's `inject-dma-error` wasn't ported), USART-level overrun, noise, stop timeout.
- **Medium: the H753ZI bench reaches the Zio headers only.** No morpho headers (CN11, CN12) are fitted: UART8 (TX only on PE1), the Lucid's USART1 RX (PA10), and PA8, PA9, PB14, PG9 are unreachable; UART5's PB13 needs SB7. Fit the headers (the 38-of-46 plan) and SB7. PA1, PA2, PA11, PA12 need rework (Ethernet, USB).
- **Low: USART2 on the F401 goes through the ST-LINK,** so break, baud and parity changes aren't checked on it. SB13/SB14 off and SB62/SB63 on would allow a direct link.
- **Low: probes are picked by VID:PID** (`0483:374b`, `0483:374e`); use serial numbers for a shared bench.
- **Low: clock checks are indirect:** `freeze`'s record matching the `const` one, plus working baud rates (and the F401 host echo). Nothing measures MCO.

---

## 2. STM32F401RE (Nucleo-F401RE)

- **Low: VOS isn't written.** 84 MHz is reached in the reset scale (scale 2).
- **Low: the USB clock isn't checked:** `pll_q` is reported, not required to be 48 MHz.
- **Low: only one supply range** (2.7–3.6 V) in `Supply`.
- **Low: HSE with a crystal is untested;** only bypass (the ST-LINK MCO) has run.
- **Low: phantom pins:** LQFP64 bonds only PD2, PH0, PH1 outside ports A to C, and no port E.
- **Low: ES0299 2.2.11 for samplers.** DMA2 can corrupt data serving AHB (GPIO, a `Sampler`) and APB2 at once. The bench's run I keeps its lanes on DMA1 (TIM3) so DMA2 serves only GPIO and memory; firmware that samples while APB2 peripherals (TIM1, SPI1, USART1/6) use DMA2 must avoid the overlap. UARTs alone are unaffected.

## 3. STM32F405RG (Foxeer F405 V2)

- **High: not run on hardware.** No Nucleo exists for it; the first run is on the Foxeer F405 V2, which needs the owner's go-ahead and a loopback plan for its pads.
- **Medium: VOS relies on its reset value.** 168 MHz needs scale 1, the reset value (RM0090 PWR_CR bit 14), so `freeze` doesn't write it; a bootloader that left scale 2 would put 168 MHz out of spec (DS8626: 144 MHz in scale 2). Fix: write the scale in `freeze`, the value from the chip file (one bit here, two on the F401).
- **Low: prefetch on revision A.** `freeze` sets PRFTEN; ES0182 2.2.1 says revision A doesn't support the prefetch queue. Later revisions are fixed.
- **Low: HSE with a crystal is untested** (the Foxeer's 8 MHz crystal).
- **Low: phantom pins:** ports E to I are handed out though LQFP64 bonds only ports A to C, PD2, PH0, PH1.
- **High: bidirectional DShot meets ES0182 2.2.19.** The Foxeer's motor timers are TIM1 and TIM8 (APB2) on DMA2, and reading the ESC replies samples GPIO (AHB), which only DMA2 reaches on the F4. DMA2 serving both at once can corrupt data. FerroHAL's `Sampler` documents it; the bench keeps lanes on DMA1 to avoid it. Before bidirectional DShot on the F405: never sample one lane while another timer's DMA2 compare traffic runs, or route the GPIO side through DMA2's memory port as the erratum describes.
- **Low: complementary outputs are untested on the F401 and F405 hardware.** The F401's only break-unit timer, TIM1, is also its only timer whose update request reaches DMA2, so the bench can't sample a CHxN output there; the H753ZI bench covers CHxN. The Foxeer's motor 4 is TIM1 CH3N.

## 4. STM32H753ZI (Nucleo-H753ZI)

### Memory and cache

- **Medium: the DTCM goes unused.** DMA1/DMA2 can't reach it, so the test firmware puts everything in AXI SRAM. A firmware that wants the DTCM needs a linker section for DMA buffers.
- **Low: MPU reads.** `check_dma_buffer` steps RNR (and restores it), so the firmware must not reprogram the MPU while a `Uart` is being opened. There's no cache-maintenance mode for buffers in cacheable memory.

### Clocks and power (`rcc_h7rm0433`)

- **Medium: no VOS0 (480 MHz).** It exists only on rev V, needs SYSCFG ODEN and its own flash table; the revision isn't read (DBGMCU IDCODE).
- **Medium: limits are the lowest common denominator:** the wide VCO is capped at 836 MHz (rev Y, DS12117 table 39 / DS12110 table 40) though rev V allows 960 MHz. Detect the revision if the headroom matters.
- **Medium: LDO only, and PWR_CR3 is written once per power-on reset.** If something already locked another configuration, `freeze` returns `SupplyTimeout`.
- **Medium: much of the clock tree isn't offered:** PLL2, PLL3, fractional mode, CSI, HSI48, HSIDIV other than 1, TIMPRE.
- **Medium: kernel clock muxes are forced** (USART16910SEL = PCLK2, USART234578SEL = PCLK1, SPI123SEL = PLL1Q, SPI45SEL = PCLK2). SPI1 to SPI3 can't be opened without PLL1's Q output.
- **Low: no IO compensation cell** (SYSCFG CCCSR); needed for fast GPIO above about 50 MHz.
- **Low: wait-state tables are transcribed** (RM0433 table 17), checked once against embassy and the manual.
- **Low: HSE with a crystal is untested;** only bypass has run.

### USART and DMA

- **Medium: no LPUART1:** `Lpuart` registers (20-bit BRR), D3 domain, DMA through DMAMUX2 and the BDMA.
- **Medium: only DMA1 and DMA2:** no BDMA (LPUART1, SPI6) and no MDMA.
- **Low: the bench's SPI checks run with the caches off,** as the Lucid does; the UART runs cover the D-cache with DMA buffers in non-cacheable memory, the SPI runs don't.
- **Low: hardware FIFOs unused** (`FIFOEN` off).
- **Low: DMAMUX overruns aren't watched** (CSR SOFx); ES0392 2.5.1 says writing CFR doesn't assert them anyway.
- **Low: phantom pins:** ports I to K are handed out though LQFP144 doesn't bond them; port H only has PH0, PH1.

### Errata

- **Medium: the ES0392 review is partial:** only USART, DMA, DMAMUX and the flash table. Review cache, system and flash ECC before relying on them.

## 5. STM32H743VI (TBS Lucid H7)

- **Medium: its own build isn't run on hardware.** The H753ZI bench runs the same die and driver code under the H753ZI glue. Build run A with the H743VI feature and run it on the H753ZI Nucleo (every pin it uses is bonded on LQFP100), or on the Lucid.
- **Low: phantom pins:** ports F to K are handed out though LQFP100 bonds ports A to E, PH0, PH1.
- Everything in section 4 applies too.

---

## 6. Open questions carried from the architecture doc

- **Kill condition for the prototype** (§11): undecided.
- **How to measure reuse** (§11): the shared UART engine runs unchanged on `usart_v2` and `usart_v4`; the per-IP `Line` is about 150 lines, the engine about 800.

## 7. Transition items

Code that exists only for the move of FerroWasp onto FerroHAL (§15). Each is marked `TRANSITION:` in its documentation and is removed once FerroWasp uses FerroHAL's native model.

- None yet. The compatibility adapter for FerroWasp's `UartRxDmaTransfer` and `UartTxDmaTransfer` will be the first, built at swap time.
