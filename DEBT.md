# FerroHAL: open debt and issues

Open items only. Finished work moves to [RESOLVED.md](RESOLVED.md), which is history and needn't be read to work on the crate.

Status as of 2026-11-10:

- **F401:** `hil/run-f401.sh` passes its twenty-one default runs, 318 checks (2026-11-12); the runs, their links and their checks are in `requirements/benches.md`.
- **H753ZI:** `hil/run-h753.sh` passes its twenty-one default runs, 532 checks (2026-11-12); run M, off the list while its CN13 cable powers the Pico, passed 11 of 11 on 2026-11-09.
- **F405RG:** `hil/run-foxeer.sh` passes 39 of 39 checks on the Foxeer F405 V2, plus the host's USB echo: its crystal and voltage scale, IMU over DMA with EXTI data ready, real SPI NOR, its battery inputs, and its motor timers and streams in the flight arrangement (TIM1 starting TIM8, 100 frames on DMA2). UARTs and real motor output haven't run on it from this bench.
- **H743VI:** `hil/run-lucid.sh` passes 51 of 51 with a card on the TBS Lucid H7 (STM32H743VIH6, TFBGA100), plus USB echo at 592 and 590 KiB/s, polled and interrupt: crystal, IMU over DMA with EXTI data ready, battery inputs, a real SD card, motor timers and streams, its UID and rev V (2026-11-10). Its DFU entry came from FerroWasp's `reboot dfu`, no BOOT hold.
- **All four chips:** `checks/run.sh` (clippy, no reachable panic, glue against metapac, host tests) passes.

| Chip | Board | IP versions | Hardware-tested |
|---|---|---|---|
| STM32F401RE | Nucleo-F401RE | `usart_v2`, `spi_v2`, `exti_v1`, `dma_v2`, `gpio_v2`, `rcc_f4` | All three USARTs, all 10 TX/RX pin entries, all 9 USART DMA routes; USART2 through a host echo on the ST-LINK VCP. All three SPIs on all 15 pin entries and all 10 SPI DMA routes. EXTI on PA12 and on every SPI SCK pin. |
| STM32F405RG | Foxeer F405 V2 | as the F401 | HSE crystal to 168 MHz, checked against the host's USB frames. USB CDC on OTG_FS. SPI1 on the IMU's pins and DMA2 streams, EXTI4 from its data-ready, SPI2 polled on the real NOR. TIM2 timebase; the watchdog started and fed. |
| STM32H753ZI | Nucleo-H753ZI | `usart_v4`, `spi_v4`, `exti_h7`, `dma_v1` + DMAMUX1, `gpio_v2`, `rcc_h7rm0433` | 7 of 8 U(S)ARTs, 24 of 46 TX/RX pin entries (Zio only), all 16 DMA1/DMA2 streams. Run A mirrors the TBS Lucid H7 (CRSF 420000, SBUS 100000 8E2 inverted, MSP 115200, its streams and clock tree). SPI1 to SPI5 on the Zio-reachable pins (SPI1 on the Lucid's pins and streams). EXTI on PD6 and on every SPI SCK pin. |
| STM32H743VI | TBS Lucid H7 | as the H753ZI | HSE crystal to 400 MHz, checked against the host's USB frames. USB CDC on OTG_FS from HSI48. SPI1 on the IMU's pins and DMA1 streams 4 and 5, EXTI2 from its data-ready. |

Severity: **High** can give wrong behaviour or blocks a planned use (FerroWasp). **Medium** is a gap someone will hit soon. **Low** is cleanup, or a limit worth knowing.

---

## 1. Crate-wide

### Correctness

- **Medium: the v2 error clear can take a good byte** ([usart_v2](src/usart_v2/mod.rs), `Line::take_errors`).
  - Clearing errors needs an SR read then a DR read; if DMA hasn't taken the byte in DR yet, the DR read consumes it. Inherent to v2 (v4 clears through ICR). A limit: "one byte may be lost per reported line error" is documented on `Uart`.
- **Low: the v2 parity clear is measured at one speed.** Run B saw the DMA already holding the bad byte with RXNE clear, and the next frame intact, one sample each way at 115200. A faster line leaves less time before the clear's DR read, which is the one-byte loss above; 420000 (CRSF) hasn't been measured.
- **Medium: parity errors can be lost while transmitting** (`usart_v2`; ES0299 2.10.2, ES0182 2.11.2).
  - In full duplex, an SR read followed by a DR write clears PE, and the TX DMA writes DR. No hardware fix. A limit: that v2 parity detection is reliable only while the receiving USART isn't transmitting is documented on `Uart`.
- **Medium: IDLE can be missed** (`usart_v2`; ES0299 2.10.1, ES0182 2.11.1).
  - IDLE isn't detected when the sender's clock runs faster than the receiver's. No workaround. A limit: that idle-ended `read_chunk` on v2 needs a timeout fallback (timer), not IDLE alone, is documented on `Uart`.
- **Low: transmit DMA errors are reported after the frame was queued.** A DMA error shows in `TxHealth` and as an error from the next `write` or `flush`.
- **Low: NDTR and the half-lap count can fall out of step** in a corner case: if interrupts keep landing between the reads, `write_count` gives up after `COUNT_TRIES` and uses the last sample.
- **Low: limits of the receive ring**, documented on `Buffers::rx`: the interrupt must be served within one ring's worth of bytes, and the reading code must never preempt `UartIrq`.
- **Low: line-error positions are approximate.** A mark sits where DMA had written when the interrupt ran, so the damaged byte is the chunk's last or near it; a full mark queue (16 between reads) reports the error without one.
- **Low: counted-wait floors are measurements** (`RX_PASS_CYCLES` 224, `TX_PASS_CYCLES` 48); without a `CycleClock`, waits run 2.5–7 times long. The HIL check "counted and clocked waits within their bounds" must pass on every new chip.
- **Low: the no-panic check proves only the paths it calls.** A new public function must be added to `checks/no-panic`.
- **Low: SPI counted waits run long without a `CycleClock`.** `PASS_CYCLES` (112) is half the fastest polling pass measured (233 cycles, H753 with caches on, measured once); the bench checks the floor with caches off, so the cached figure isn't rechecked. Waits run up to about 3 times long on the F401 and 6 on the uncached H753.

### API gaps

- **Low: the flight boards' I2C has carried bytes only on the Lucid.** Its barometer answers at `0x76` on I2C2 (PB10/PB11); the SpeedyBee's SPA06-001 and the Foxeer's I2C1 wiring use the same open-drain pins, but nothing has answered.
- **Low: the I2C scope is master, 7-bit, no SMBus, no DMA.** SMBA isn't routed; slave mode or 10-bit addressing isn't asked for. A held-low line is [`Error::Busy`], not repaired: nine-clock recovery is firmware's.
- **Medium: USB sits outside the no-panic proof.** `synopsys-usb-otg` has about 30 panic/unwrap sites and unbounded register spins; behind the `usb` feature, the check covers only FerroHAL's clock and constructor. Flight firmware should keep USB away from critical paths.
- **Medium: USB is device-mode CDC on OTG_FS, tested on the H753ZI, the Foxeer's F405 and the Lucid's H743.** No OTG_HS, no host mode; the F401's Nucleo has no USB connector.
- **Medium: the frame format is limited:** 8 data bits (parity as a ninth); no 7/9-bit data, RTS/CTS, half-duplex, RS-485 DE or LIN.
- **Medium: no timer hand-over to rtic-monotonics** (`docs/design/layering.md` hook 5, `docs/design/ferroforge-seam.md` foot-gun 1): `Tick` and `Timebase` run timers, but rtic-monotonics 3 can't take a FerroHAL token as its monotonic.
- **Low: only the trigger routes in use are in the glue:** TIM1→TIM3 everywhere, TIM1→TIM8 where TIM8 exists, TIM3→TIM2 on the F4s (run R). `check_glue.py` can't check them (no such metadata table); each follows RM0090 table 94, RM0368 table 54 or RM0433 table 336, and hardware exercises it.
- **Medium: timer features still missing:** input capture, dead time, the break input (FerroWasp's reply window).
- **Low: the watchdog's LSI constant is one measurement, and windows are nominal.** `start_with_lsi` scales to a per-board constant, but the LSI drifts with temperature and supply, so the timeout follows; `start_windowed` counts at 32 kHz.

- **Low: arming and staging lanes takes time.** 14 µs for two lanes on the F401 at 84 MHz; a frame scheduler must allow for it.
- **Low: sampler rates.** The bench samples at about 12 times the DShot600 bit rate (7 MHz on the F401, 7.1 MHz on the H753) without loss; faster rates are untested.
- **Low: a tick or timebase rate must divide the timer clock exactly.** `Tick` and `Timebase` refuse a rate they would have to round (`ConfigError::Inexact`) rather than drift.
- **Low: some timers share their update vector** (the F4's `TIM1_UP_TIM10`, the H7's `TIM8_UP_TIM13`). `Tick::on_interrupt` returns `false` for another's flag; firmware calls every sharer's handler.
- **Low: `verify()` can't check task priorities.** It checks a UART's vectors are enabled at one priority; "above every task that uses it" is the application's job.
- **Low: no `free()` returns its pins,** only the token: no second role on its pins ([free.md](docs/design/free.md)).
- **Low: DMA double buffering and FIFO threshold mode are untested,** and unreachable outside the crate since W6b2 made `Channel::start` internal.
- **Medium: the ADC is regular conversions by software start only.** No timer trigger, injected channels, watchdogs, oversampling, or differential inputs; ADC2 isn't offered, nor the H7's `_C` pads. An `AdcScan` holds its ADC for good, and one sample time serves every channel.
- **Low: the H7's ADC runs at a fixed 4 MHz,** per_ck (HSI) over 16, FerroWasp's speed, with BOOST left at its reset value, which suits that clock on both silicon revisions. ES0392 2.9.8 limits ADC3's resolution while the LSE runs; nothing here starts it. The Nucleo's bench reads 16 bits to within a few hundred counts on the rails, with no accuracy figure beyond that.
- **Medium: the SPI is master, full duplex, 8-bit only.** No slave mode, simplex or half-duplex, 16-bit frames, hardware NSS or CRC. That holds for both `Spi` (DMA) and `BlockingSpi` (polled).
- **Medium: `BlockingSpi` holds the CPU for the whole transfer.** One byte in flight, polled, with gaps between bytes (1.5 times the wire time at 5.25 MHz on the F401, 1.6 at 6.25 MHz on the H753). Fine for FerroWasp's NOR at 10 MHz, not for long transfers at high priority.
- **Low: SPI bit order is unchecked on hardware.** A MOSI-to-MISO loop returns the bytes whichever bit goes first, so `BitOrder::LsbFirst` is only shown to transfer.
- **Low: EXTI covers GPIO lines 0 to 15 only,** as interrupts: no event mode (EMR), and none of the internal lines (PVD, RTC, USB wake-up and the like).
- **Low: EXTI owns the whole SYSCFG token.** It needs only EXTICR, but SYSCFG also holds the H7's I/O compensation cell and the F4's memory remap, which later drivers will want.
- **Low: FIFO errors are advisory.** On the TX stream they're cleared and ignored (FerroWasp's policy); on the RX ring they're `DmaOverrun`. Confirm both on hardware.

### Architecture and process

- **Medium: the routing generator doesn't exist** (`docs/design/api-surface.md`, `docs/design/scope.md`). Glue rows are hand-written; `tools/check_glue.py` checks them against metapac (U(S)ARTs, SPIs, timers, USB, ADCs, SDMMC, DMA and EXTI vectors: 127 facts on the F401, 173 on the F405, 180 on the H743, 184 on the H753) by reading the rows as text, so their layout must stay as it is. The metadata doesn't hold the USB FIFO depth, endpoint count or core kind, the clock limits or the memory map, so those stay checked by hand against the manuals.
- **Medium: five source files each serve two IP versions** (`docs/design/structure.md`, `docs/design/requirements-system.md`). [dma_stream.rs](src/dma_stream.rs) serves `dma_v1` and `dma_v2`, [exti.rs](src/exti.rs) `exti_v1` and `exti_h7`, [timer.rs](src/timer.rs) `timer_v1` and `timer_v3`; the engines [uart/stream_dma.rs](src/uart/stream_dma.rs) and [spi/stream_dma.rs](src/spi/stream_dma.rs) serve two IP versions each through their `Line`/`Unit`. Decide whether a module's hash covers shared sources, or split once they diverge.
- **Medium: the feature hierarchy departs from `docs/design/structure.md`.** Chip features turn on their family marker, and two chip features together are caught only by a `compile_error!` listing every pair (six at four chips).
- **Low: the bench isn't on FerroForge yet.** Decided (2026-11-02): no fork is needed, since FerroForge's macros don't depend on a HAL and FerroHAL provides the RTIC device module; only its CLI backends name stm32f4xx-hal and stm32h7xx-hal. When FerroWasp's swap nears, add a FerroHAL backend to the FerroForge CLI and move the bench runs onto FerroForge tasks, which should cut 15-25 % of the run binaries' lines and prove the pair together.
- **Medium: every driver has requirements, but tracing isn't enforced per file** (`docs/design/requirements-tooling.md`). `every_file_traced` in `reqtool.toml` stays off, because reqtool applies it to the bench crates under `hil/` as well as the library, and calls every file safety-critical. Neither the catalog links nor the diff analysis record is built.
- **Medium: no requirement covers what's listed elsewhere in this file as missing or untested,** such as DMA double buffering or SPI bit order. A requirement says only what's measured.
- **Medium: a UART fault still has no evidence** (`UARTDMA-009`): no bench makes a DMA stream refuse to stop. The owner set it aside until after the swap (2026-11-03): file it under the swap's follow-ups, not as a blocker.
- **Low: kernel clock equals bus clock.** True on F4; on H7 only because `freeze` forces the kernel muxes. Add a kernel-clock field to `Info` before offering other muxes.

### GPIO

- **Low: a bonded port still splits into every pin.** `Port::split` hands out all sixteen pins, so an unbonded one (PB11 on the F401's LQFP64, a second PH pin on the H7s) can be split out as GPIO though no routing row may name it; `BONDED` and `check_glue.py` enforce the rows, not the split. A per-package `Parts` would remove the fields.
- **Medium: an I2C wiring accepts a push-pull pin:** both `into_alternate` forms return the same type ([gpio_v2](src/gpio_v2/mod.rs)), and push-pull NACKs every transfer (RESOLVED, 2026-11-08). Fix: an open-drain type state.
- **Low: missing GPIO features:** an open-drain output, runtime mode switching.

### Test harness (`hil/`)

- **Medium: the H753ZI bench reaches the Zio headers only.** No morpho headers (CN11, CN12): UART8 TX, the Lucid's USART1 RX, PA8, PA9, PB14 and PG9 are unreachable, and UART5's PB13 needs SB7; PA1, PA2, PA11, PA12 need rework.
- **Low: USART2 on the F401 goes through the ST-LINK,** so break, baud and parity changes aren't checked there; SB13/SB14 off and SB62/SB63 on would allow a direct link.
- **Low: clock checks are indirect:** `freeze`'s record matching the `const` one, plus working baud rates (and the F401 host echo). Nothing measures MCO.
- **Low: `hil/run-i2c-sim.sh` with two probes attached needs both `F401_PROBE` and `H753_PROBE`;** `PROBE_RS_PROBE` would send both runs to one probe, as `hil/probe.sh` ranks it first.

---

## 2. STM32F401RE (Nucleo-F401RE)

- **Low: only one supply range** (2.7–3.6 V) in `Supply`.
- **Low: HSE with a crystal is untested;** only bypass (the ST-LINK MCO) has run.
- **Low: ES0299 2.2.11 for samplers.** DMA2 can corrupt data serving AHB (GPIO, a `Sampler`) and APB2 at once. The bench's run I keeps its lanes on DMA1 (TIM3) so DMA2 serves only GPIO and memory; firmware that samples while APB2 peripherals (TIM1, SPI1, USART1/6) use DMA2 must avoid the overlap. UARTs alone are unaffected.

## 3. STM32F405RG (Foxeer F405 V2)

- **Low: UART4 and UART5 have never run on `usart_v2`.** The F401 doesn't have them, and the F405 shares its driver with the F401, so no separate F405 UART bench is planned (decided 2026-11-02). Their glue is checked, and UART4 is the Foxeer's MSP port, so the swap's bench run will exercise it.
- **Low: no frame has reached a Foxeer motor pad.** The motor check runs with outputs off; real DShot on the pads waits (decided 2026-11-02). The same lanes are read back by samplers on the Nucleos.
- **Low: the bench's NOR commands are its own copy.** They follow FerroWasp's `spi_nor` driver, which compiled unchanged over `BlockingSpi` in a scratch crate but hasn't itself run over FerroHAL. The bench can't depend on it from a public repo. The IMU setup is copied the same way.
- **Low: FerroWasp's F405 image hasn't been seen through a DFU `leave`;** since the swap it enables FerroHAL's `rt` (FerroHAL `b6d8e84`), which should clear the VTOR trap.
- **Low: a Foxeer firmware that fails before USB can't say why.** It restarts into DFU, which shows only that something failed.
- **Low: prefetch on revision A.** `freeze` sets PRFTEN; ES0182 2.2.1 says revision A doesn't support the prefetch queue. Later revisions are fixed.
- **Medium: reading a pin on DMA2 while DMA2 also serves APB2 meets ES0182 2.2.19.** The waveform generator can now turn a lane round to read its pin, which is what bidirectional DShot needs, and FerroWasp doesn't use it yet. The Foxeer's motor timers are TIM1 and TIM8 on DMA2, and only DMA2 reaches GPIO on the F4, so reading replies shares DMA2 with anything on APB2, such as SPI1's IMU streams. DMA2 serving both at once can corrupt data. Before bidirectional DShot flies on an F4, the two need keeping apart.
- **Low: complementary outputs are untested on the F401 and F405 hardware.** The F401's only break-unit timer, TIM1, is also its only timer whose update request reaches DMA2, so the bench can't sample a CHxN output there; the H753ZI bench covers CHxN. The Foxeer's motor 4 is TIM1 CH3N.

## 4. STM32H753ZI (Nucleo-H753ZI)

### Memory and cache

- **Medium: the DTCM goes unused.** DMA1/DMA2 can't reach it, so the test firmware puts everything in AXI SRAM. A firmware that wants the DTCM needs a linker section for DMA buffers.
- **Low: MPU reads.** `check_dma_buffer` steps RNR (and restores it), so the firmware must not reprogram the MPU while a `Uart` is being opened. There's no cache-maintenance mode for buffers in cacheable memory.

### Clocks and power (`rcc_h7rm0433`)

- **Medium: no VOS0 (480 MHz).** It exists only on rev V, needs SYSCFG ODEN and its own flash table; `freeze` doesn't read `signature::revision`.
- **Medium: limits are the lowest common denominator:** the wide VCO is capped at 836 MHz (rev Y, DS12117 table 39 / DS12110 table 40) though rev V allows 960 MHz. `signature::revision` tells, if the headroom matters.
- **Medium: LDO only, and PWR_CR3 is written once per power-on reset.** If something already locked another configuration, `freeze` returns `SupplyTimeout`.
- **Medium: much of the clock tree isn't offered:** PLL2, PLL3, fractional mode, CSI, HSI48, HSIDIV other than 1, TIMPRE.
- **Medium: kernel clock muxes are forced** (USART16910SEL = PCLK2, USART234578SEL = PCLK1, SPI123SEL = PLL1Q, SPI45SEL = PCLK2). SPI1 to SPI3 can't be opened without PLL1's Q output.
- **Low: no IO compensation cell** (SYSCFG CCCSR); needed for fast GPIO above about 50 MHz.
- **Medium: HSI48 runs untrimmed for USB.** Full speed asks for 48 MHz to 0.25 %. The CRS can trim HSI48 from USB start-of-frame, but ES0392 2.2.3 says that doesn't work on revision Y, which `signature::revision` tells. FerroWasp also runs it untrimmed. Enumeration and 64 KiB of echo pass on the bench; add CRS for revision V if a host proves picky.
- **Low: wait-state tables are transcribed** (RM0433 table 17), checked once against embassy and the manual.
- **Low: HSE with a crystal is untested;** only bypass has run.

### USART and DMA

- **Medium: no LPUART1:** `Lpuart` registers (20-bit BRR), D3 domain, DMA through DMAMUX2 and the BDMA.
- **Medium: only DMA1 and DMA2:** no BDMA (LPUART1, SPI6) and no MDMA.
- **Low: the bench's SPI checks run with the caches off,** as the Lucid does; the UART runs cover the D-cache with DMA buffers in non-cacheable memory, the SPI runs don't.
- **Low: hardware FIFOs unused** (`FIFOEN` off).
- **Low: DMAMUX overruns aren't watched** (CSR SOFx); ES0392 2.5.1 says writing CFR doesn't assert them anyway.

### Errata

- **Medium: ES0392 2.2.10 exposes AXI SRAM DMA reads on rev-Y H7s.** DMA buffers are AXI SRAM (bench: its top 16 KiB; `chip::DMA_RAM`); on revision Y/W a controller delaying acceptance while another read is issued can see corruption (absent on X and V). `AXI_TARG7_FN_MOD.READ_ISS_OVERRIDE` fixes it at a throughput cost and metapac lacks the register. `signature::revision` reports the code at the end of every bench run's log: the Lucid and the Nucleo-H753ZI are both revision V (0x2003); a Y/W part needs the override. `docs/design/h7-errata.md`.
- **Low: a flash driver must start from ES0392 2.2.6/2.2.7/2.2.18.** `BFAR` may hold the previous access's address on a double ECC error, so only `FLASH_ECC_FA1R/FA2R` is usable; `FAxR` may miss the second of two consecutive errors (no workaround), and flash CRC is invalid at sector 7's last address. Nothing reads flash status yet.

## 5. STM32H743VI (TBS Lucid H7)

- **Low: SDMMC has run on one card** (a 16 GB SanDisk in the Lucid) and the empty slot, which reports `NoCard`; a CRC failure and a card pulled mid-transfer are untested.
- **Medium: SDMMC's limits.** Polled through the FIFO (interrupts may preempt, flow control pausing the bus), up to 25 MHz, 4-bit, 3.3 V, high-capacity v2 cards; no DMA, no card-detect pin.
- **Low: no frame has reached a Lucid motor pad, and its UARTs run only as the H753ZI's.** Same die and driver under the H753ZI's glue, with the H743 routes pinned by a test; UART8 (its ESC telemetry port) has run on neither.

- Everything in section 4 applies too.

---

## 6. Transition items

Code that exists only while FerroWasp's transports run through the compatibility adapters (ADR-005). Each is marked `TRANSITION:` in its documentation and is removed once the transports move to FerroHAL's native model.

- FerroWasp's UART and SPI adapters, in its `ferrowasp-stm32-critical`, marked `TRANSITION:` there. They go when its transports move to FerroHAL's native model. FerroHAL itself holds no transition code.

## 7. Deferred: no bench can prove it yet

Built, and checked by the compiler, host tests or the no-panic link, but the
behaviour they change has not run on hardware. They stay open and rank below
every testable item above.

- **Lowest: the rig cannot vary the conditions several ES0392 rules depend on.** One MPU/cache shape beyond the matrix, one package and one silicon revision per chip: the HCLK/8 SysTick path, WWDG, option-byte/RDP flows and flash ECC go untested, so those errata stay firmware rules, not bench facts.
