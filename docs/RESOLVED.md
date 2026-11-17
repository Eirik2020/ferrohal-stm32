# FerroHAL: resolved debt

History: items moved out of [DEBT.md](DEBT.md) once done. The chip and feature sections continue in [RESOLVED-chips-and-features.md](RESOLVED-chips-and-features.md); crate-wide entries up to 2026-11-09 in [RESOLVED-crate-wide-to-2026-11-09.md](RESOLVED-crate-wide-to-2026-11-09.md). Limits a fix left behind stay in DEBT.md as open items. Newest first within each section. Section numbers (§) refer to the architecture map, dissolved into `docs/design/` topics on 2026-11-10; the numbers cite it as it was.

## Crate-wide

- **API review N2: no module prefix, with recorded exceptions** (2026-11-17; owner's
  pick A). `spi::SpiParts`, `i2c::I2cParts`, `adc::AdcWiring`, `adc::AdcDma` and
  `reset::ResetCause` are `spi::Parts`, `i2c::Parts`, `adc::Wiring`, `adc::Dma` and
  `reset::Cause`. `design/naming.md` records the rule and its exceptions: the main
  driver types and their refusals, and the routing traits a glue file imports beside a
  namesake (`SpiTxDma`, `SpiRxDma`, `AdcPin`). The UART's names (`UartIrq`, `UartRx`,
  `UartTx`, `UartInterrupts`) and `SpiInterrupts` (D13) move with the single-owner
  redesign. Spawned by `af65cad` onward.

- **API review N1: refusal aliases are `<What>Refused`** (2026-11-17; owner's pick A).
  Four patterns (`<Type>Refused`, `<Type>NewRefused`, `<Type>Refusal`, a `Result`
  alias) become one: `LaneRefusal`, `ComplementaryLaneRefusal`, `SdmmcNewRefused`,
  `UsbNewRefused` and `SamplerNewRefused` are `LaneRefused`, `ComplementaryLaneRefused`,
  `SdmmcRefused`, `UsbRefused` and `SamplerRefused`; the `TwoWayLaneAssembly` result
  alias is `TwoWayLaneRefused`; SPI and I2C gain `SpiRefused`, `BlockingSpiRefused` and
  `I2cRefused`, so their constructors lose `#[allow(clippy::type_complexity)]`. The
  UART's alias comes with its single-owner redesign. Spawned by `c6c5d23`, `ee05895`.

- **API review N3: one quantity, one word** (2026-11-17; owner's pick A). I2C said
  speed, SPI and SDMMC frequency, the PWM `new(rate)` but `set_frequency`, and the UART
  a bare `u32` baudrate. Now, as `design/naming.md` records: I2C's `Config.speed`,
  `Timing::speed` and `ConfigError::Speed` are `frequency`, `frequency()` and
  `Frequency`; `Pwm::set_frequency` and `frequency` are `set_rate` and `rate`;
  `uart::Config.baudrate` is a `Hertz`. "Speed" is left to GPIO slew. Spawned by
  `ea7a9f8`, `99e6404`, `cdfe190`.

- **API review N4: the erased DMA stream is `dma::AnyStream`** (2026-11-17; owner's pick
  A). `dma::Channel` was an erased stream in a module where "channel" also meant the
  F4's CHSEL request, and the timers' compare channels beside it. It is
  `dma::AnyStream`; `StreamInstance::into_channel` is `into_any` and the crate's
  `claim_channel` is `claim`. Spawned by `14f7492`.

- **A Foxeer run that stops before USB says why** (2026-11-17; owner's go-ahead). It
  restarted into DFU silently: the board has no probe. Before every stop - a step it
  cannot go on from (the source line), a panic (its line), a hard fault, a watchdog
  reset, no host within 30 s - `usb_bench` now writes a four-word stop record to the top
  32 bytes of SRAM, which the Foxeer's `memory.x` keeps out of RAM and the ROM
  bootloader does not use (AN2606: its 8 to 16 KiB start at 0x2000_0000).
  `hil/flash-dfu.sh` reads it over DFU (`STOP_RECORD`, set by `run-foxeer.sh`) when a run
  fails and the board sits in DFU, and the next run that reaches USB reports it as a
  `FAIL` and clears it. The Lucid keeps no record yet (`STOP_RECORD` = `None`). Not yet
  on the Foxeer: that the ROM leaves the words and lets DFU upload them is the first
  Foxeer run's to show. Spawned by `926b210` (2026-11-02).

- **A compare lane refuses the duty step the timer misses** (2026-11-17; owner: measure
  first). ES0392 2.16.2, ES0299 2.6.3 and ES0182 2.7.3 name a compare match at
  CNT = CCR = 0 after one at CNT = CCR = ARR. Measured on the H753 (run J, TIM3 at
  DShot600, 200 MHz counter, period 333): after a step to 0 the next duty started one
  period late for every duty from the top down to 8 counts below it, and kept time from
  12 counts below; a step from near the top to a quarter duty, and from a quarter duty
  to 0, kept time. `CompareLane::arm` now refuses a duty within `Pwm::top_margin`
  (100 ns of counter clock, at least one count; the margin over the 60 ns measured is
  inferred room for DMA latency) followed by 0, or ending a waveform, with
  `Error::TopToZero` (new requirement PWM-009). Run J checks the refusal and that the
  same waveform just clear of the margin keeps time. Measured on `timer_v3` only; the
  refusal is the shared lane code, so `timer_v1` refuses the same. Spawned by the ES0392
  row review (2026-11-17).

- **The H7 USB port sends zero-length packets by ES0392 2.25.7** (2026-11-17; owner,
  pin a fork). `synopsys-usb-otg` 0.4.0 set CNAK and EPENA together for an empty IN
  packet, which on the H7's core can put a wrong packet on the bus. FerroHAL now takes
  0.4.0 from a fork pinned by exact revision (`Eirik2020/synopsys-usb-otg`, branch
  `ferrohal-es0392-2.25.7`, `902ebe9`, one commit on upstream's v0.4.0): with the `hs`
  feature, `write` sets SNAK, waits 20 AHB clocks, sets EPENA, waits 15, then sets
  CNAK. FerroHAL could not wrap it, since the crate builds its bus only inside its own
  allocator. Proof is the H753's CDC run (run M) on the merge gate. Spawned by the
  ES0392 row review (2026-11-17).

- **API review N14, N15 and N16 (EXTI half): one verb per act** (2026-11-17). Giving a
  token back is `free` everywhere: `Exti::release` is `Exti::free`. Abandoning a
  transfer is `abort` everywhere: `Sampler::stop` is `Sampler::abort` (`Tick::stop` and
  `Pwm::stop` stop a counter, not a transfer, and stay). Interrupt enables are
  `mask`/`unmask`: `Exti::listen`/`unlisten` are `unmask`/`mask`, so `listen` means
  capture only (`CompareLane`, `Awg`). N16's UART half (`Line::mask`) moves with the
  UART single-owner redesign (owner, 2026-11-17). The rename showed that
  `dma::fault::release` had counted as called only through `Exti::release`'s name; it
  is listed in `checks/no-panic/uncalled.txt` beside `stick`. Spawned by `b55c640`,
  `ea7a9f8`, `1ca19a9`.

- **The F401 bench's SPI SCK speed was never at fault** (2026-11-17). The ES0299 row
  review logged that runs E to G drive SCK at reset speed with APB2 at 84 MHz, outside
  2.11.4's bound. Wrong: the board's `sck()` sets every SCK and MOSI pin to high speed,
  which the sheet allows up to 84 MHz. The item was the review's misreading of the
  source; nothing changed. Spawned by the ES0299 row review (2026-11-17).

- **The F4 I2C has its two errata workarounds** (2026-11-17). ES0299 2.9.1, ES0182
  2.10.1: a spurious BERR no longer fails the transfer (the shared driver's change with
  the H7's 2.19.4). ES0299 2.9.3, ES0182 2.10.3: after a misplaced Stop the peripheral
  cannot start until SWRST; `i2c_v1`'s `recover` now resets through SWRST and writes
  CR2's FREQ, CCR and TRISE back, where it cycled PE. Every recovery takes that path, so
  the F4 I2C live runs (NACK, held line, timeout) exercise it. Spawned by the ES0299 and
  ES0182 row reviews (2026-11-17).

- **The H7 I2C has its three ES0392 workarounds** (2026-11-17). 2.19.4: a BERR in
  controller mode can be spurious and leaves the transfer running, but `i2c_v2` failed
  the transfer with `Error::Bus`; the shared driver now clears BERR, counts it in
  `Health::bus_errors` and carries on, on both IP versions (`i2c_v1` too, for ES0299
  2.9.1 and ES0182 2.10.1). 2.19.3: `timing` refuses a kernel clock slower than the
  mode's data setup (4, 10, 20 MHz). 2.19.5: on revisions Y and W a read over 255 bytes
  reloads one byte at a time before its last chunk, checked through `ES0392_2_19_5`. Host
  tests cover the floor and the chunking; a spurious BERR cannot be provoked, and both H7s
  here are revision V, so 2.19.5's path runs on no bench (DEBT §4, another silicon
  revision). Spawned by the ES0392 row review (2026-11-17).

- **An F405 revision A matches its own errata** (2026-11-17). ES0182 2.2.2: revision A
  reads REV_ID 0x2000, which `Erratum::affects` took for a revision newer than the
  sheet's newest, so revision A's errata read as unaffected and `freeze` set PRFTEN
  there again (2.2.1). Die 0x413's `revision()` now maps 0x2000 to 0x1000; host tests
  cover the mapping and that 2.2.1 then applies. No revision-A part is on the rig, so
  no bench proves it (DEBT §4, another silicon revision). Spawned by the ES0182 row
  review (2026-11-17).

- **API review S7 and N6: timer leftovers and small naming drift** (2026-11-17).
  `Tick::free` repeated `stop`'s three writes; it calls `stop` now (S7's F4 RCC half
  went with U2). N6: the owned timer tokens were held in `_timer` fields that were then
  used (`clippy::used_underscore_binding`, three hits); they are `timer`. The private
  UART alias is `usart_ip`, like `adc_ip`, `i2c_ip` and `spi_ip`; the ADC's `vdda_mv`
  parameters are `vdda_millivolts`, like `vdda_millivolts()`; the unused
  `i2c_v1::Block`/`i2c_v2::Block` aliases are gone. Spawned by `ea7a9f8`, `cdfe190`,
  ADR-002's `free`.

- **API review U2, S7 (F4 half) and U9: the RCC modules share what is the same**
  (2026-11-17). `Hse`, `HseMode`, `PllSource`, `AhbPrescaler`, `ApbPrescaler` and
  `RccBit` were written twice, identically; the F4 hand-wrote its `enable` and `reset`
  as four-arm matches where the H7 used `bus_registers!`; and eleven constructors wrote
  `rcc.enable(x); rcc.reset(x)`. The new private `rcc_shared` module holds the six types
  and `bus_registers!`, which both RCC modules now expand, and which adds `Rcc::open`
  (enable, then reset); every constructor calls it. Each RCC module keeps its register
  encodings, which metapac types per family. S7's other half (`Tick::free` repeating
  `stop`) stays open. Spawned by `af65cad`.

- **API review U7: no unsafe casts for the buffer check** (2026-11-17).
  `chip::check_dma_buffer` took `&[u8]`, so the ADC and the timer lanes kept three unsafe
  slice-to-bytes casts only to call it. It is now generic over `&[T]` (the span is
  `size_of_val`), as is the `within` helper behind it, and the three casts and their
  `unsafe` blocks are gone. Spawned by `ea7a9f8`, `a8a3a05`.

- **Every errata entry has a verdict** (2026-11-17). ADR-010 transcribed every row of
  ES0182, ES0299 and ES0392 (2026-11-15) with 265 left `Unreviewed`; each now carries
  its handling, read against the sheet and the source, in `docs/errata/`. The rows that
  affect FerroHAL and lack a workaround are DEBT items (F405 revision A's REV_ID, the I2C
  on both families, a compare lane's duty step, the H7's zero-length USB packet).
  Spawned by ADR-010's transcription.

- **API review U8: wait constants have one home** (2026-11-17). `STOP_SPINS` was defined
  four times (10 000 three times, 100 000 in the ADC), `PASS_CYCLES` twice and
  `MARGIN_CYCLES` three times. Now `dma::STOP_SPINS` (10 000, the ADC's stream stop
  included) and `wait::PASS_CYCLES`, `wait::MARGIN_CYCLES`. The H7 ADC's own
  `STOP_SPINS` bounds its ADSTP/ADDIS, not a stream, and stays. Spawned by `af65cad`,
  `99e6404`.

- **API review D11: one wait mechanism** (2026-11-17). Besides `wait::Deadline`, 17 raw
  spin loops in nine files (both RCC modules with a private `wait`, both ADCs, SDMMC,
  IWDG, `i2c_v1`, `spi_v2`, DMA) counted their own polls. They now all go through
  `wait`: `Deadline::passes(n)` is the clockless bound, `wait::spin` its shorthand, and
  `wait::poll` a wait that can end with a value (an error met on the way). Bounds and
  behaviour are unchanged: each still polls its `*_SPINS` count. Left as they are: the
  `*_TRIES` retries (SDMMC power-up and busy), which resend a command each pass, and the
  H7 ADC's calibration, which already waits in milliseconds. Proven on the merge-gate
  benches of PR #49. Spawned by `af65cad`.

- **API review D10: every wiring describes itself** (2026-11-17). USB and SDMMC were the
  two wirings without `RESOURCES`. USB's came with D9; `sdmmc::Wiring::RESOURCES` lists
  the instance and its six pins (new `Signal::Ck`, `Cmd`, `Data(n)`). Spawned by
  `9bd9d1e` and `4237048`, which did not follow `2e7f7fe`'s descriptors.

- **API review D9: USB's data pins come from the glue** (2026-11-17). `usb.rs` hard-coded
  PA11/PA12 in AF10. glue-gen now reads OTG_FS's DM and DP routes from metapac (one each
  on every part, else it fails) and `usb_fs!` defines `chip::UsbDm` and `chip::UsbDp`.
  USB also gained a `usb::Wiring { usb, dm, dp }` with `RESOURCES` (`Signal::Dm`, `Dp`),
  taken by `UsbFs::new` and handed back by `free` and by a refusal: D10's USB half and
  N8's "USB has no wiring". Spawned by `9bd9d1e`.

- **API review K4: one list of shared sources** (2026-11-17). ADR-003 rule 4 named two,
  the 2026-11-13 debt sweep five, and the review's own test nine. `design/naming.md` now
  holds the one list, the four `#[path]` aliases (`iwdg.rs`, `exti.rs`,
  `dma_stream.rs`, the timer split), says it is complete, and says why the neutral
  engines serving each IP version through its register wrapper are not exceptions.

- **API review K1: the bootloader jump is a recorded exception** (2026-11-17).
  `reset::enter_bootloader_if_requested` masks interrupts, clears SysTick and every NVIC
  line, moves VTOR and, on the F4, writes RCC and SYSCFG without their owners: against
  the README, `design/scope.md` and `design/ownership.md`, unrecorded. All three now name
  it as the single exception, sound because the jump is one-way and runs before any
  owner exists. Spawned by `036a7d2`.

- **EXTI takes only SYSCFG's EXTICR part** (2026-11-17; owner's option A,
  2026-11-16). The new `syscfg` module's `SYSCFG::split(&mut Rcc)` turns on SYSCFG's
  clock and returns `Parts { exticr }`; `EXTI::split(exticr)` takes that part alone and
  no longer needs `&mut Rcc`. The F4's memory remap and the H7's I/O compensation cell
  get parts of their own when a driver needs them. Spawned by `b55c640`, EXTI written
  as SYSCFG's only user.

- **An I2C wiring takes only an open-drain pin** (2026-11-17; owner's option A,
  2026-11-16). `Alternate<AF, OT = PushPull>` carries the output type:
  `into_alternate` returns `Alternate<AF>` and `into_alternate_open_drain`
  `Alternate<AF, OpenDrain>`. The generated `SclPin` and `SdaPin` rows name the
  open-drain form only (`pins!(SclPin<I2C1>, OpenDrain: ...)`), so a push-pull I2C
  wiring no longer compiles. Push-pull users are unchanged. `checks/no-panic` built its
  I2C wirings push-pull, exactly the fault; it now uses the open-drain form. Spawned by
  GPIO's single alternate type (`af65cad`); found by run S (`cb5e339`).

- **DMA double buffering and FIFO thresholds are gone** (2026-11-16). No driver used
  them, no run proved them, and since W6b2 nothing outside the crate could reach them.
  `Transfer` loses `double_buffer` and `fifo`, `FifoMode`, `FifoThreshold`, `Target` and
  `Channel::current_target` go, and every transfer runs in direct mode as all of them
  already did.
- **The ADC's `Unit::new` is crate-only** (2026-11-16), like I2C's and SPI's: it builds a
  unit from raw registers for the chip glue. `checks/no-panic/uncalled.txt` no longer
  lists it.

- **The no-panic check covers every public function** (2026-11-16). It proved only the
  paths `checks/no-panic` called, and nothing checked that list.
  `tools/check_no_panic_coverage.py`, a `checks/run.sh` step, now requires every
  `pub fn` reachable from outside the crate to be called there or listed in
  `checks/no-panic/uncalled.txt` with a reason (three: the ADC's two raw `Unit::new`
  and the fault-injection hook). It works from the source text on stable Rust (the
  owner's choice over rustdoc JSON on nightly), so a method counts by its name. It found
  27 uncalled; the 24 now called (clock solving, I2C timing, errata, the ADC's
  calibration maths, `set_frequency`, `complementary`, `Awg::linked` and
  `independent`, `set_rate` among them) link on every chip with no panic path.

- **The clock trees are measured against the host** (2026-11-15). H753 run M and
  Discovery run B count CPU cycles over 1000 USB start-of-frames, a clock independent of
  the board. Discovery: 168.037 MHz (220 ppm). H753 from HSE: 398.3 to 398.5 MHz, because
  its input, the ST-LINK-V3's MCO, is 0.4 % low; run M moved from HSI (which read 0.43 %
  high) to HSE, so the check sees the tree from a known input. The F401 part stays
  open (no USB device).

- **The `usart_v2` parity clear is measured above 115200** (2026-11-15). F401 run B now
  checks it at 115200 and 1 Mbaud, both directions, and both hold. The faster rates
  fail and opened a new item (DEBT, Library code: above 1 Mbaud, a lone parity error).

- **The SPI runs with the caches on** (2026-11-15). H753 run H turns both caches on after
  its caches-off suites and runs SPI2's suite again: 30 of 30 pass, and the fastest
  polling pass is 228 cycles over 2258 passes, so `PASS_CYCLES` (112) keeps its factor of
  two (the source's note says 233, the one earlier measurement).

- **`hil/pico-i2c-sim.sh` no longer needs both probe variables** (2026-11-15). With
  several probes attached and no `*_PROBE` set, `hil/probe.sh` now picks the probe whose
  die has the bench's DEV_ID, and the simulator script clears `PROBE_RS_PROBE` for each
  of its two runs. Measured: both runs S pass with no probe variable set.

- **`Adc`, `AdcScan`, `Sdmmc` and `UsbFs` hand their parts back** (closed 2026-11-13, the
  DEBT item removed 2026-11-15). Each gained `free()` with the rest (`docs/design/free.md`,
  rule 0); the item outlived the work.

- **SPI bit order and clock phase are checked on the wire** (2026-11-15). A MOSI-to-MISO
  loop returns the bytes either way, so `LsbFirst` was only shown to transfer (DEBT since
  `e6c2362`, 2026-10-31; the fix it named needed one more link). No link was needed:
  SPI1's SCK and S1's MISO end share port A on both Nucleos, so the timer-paced sampler
  reads the wire. SPI-010 holds on `spi_v2` (F401 run E) and `spi_v4` (H753 run F).

- **UARTDMA-009 has a method, and `free` no longer hands out buffers under a running
  stream** (2026-11-15). No bench could make a stream refuse to stop (DEBT, since
  2026-11-03). The owner chose a bench-only `fault-injection` feature: `dma::fault`
  makes a stream ignore stop requests. Run T on the H753 proves the UART fails closed
  in the stuck direction and keeps the other. Building it found that every DMA
  driver's `free` ignored a failed stop yet returned its buffers as `&'static mut`;
  they now refuse with the handle (API-003, owner's choice of shape).

- **RESET-004 holds on the H7** (2026-11-15). Run N asks for the ROM across resets but
  ends with the core halted, so no DFU device ever appeared and the cause stayed open
  (`57ae1b3`, 2026-11-13). Run Z ends in the ROM: a new DFU device with the die's IDCODE
  appears on CN13 (`200364500070`; the H7 ROM's serial is not the F4's UID-derived one),
  and the chip is erased after. The bootloader is 0x91, past AN2606's PWR_CR3 lock.

- **Complementary outputs have a requirement, proven on both timer IP versions**
  (2026-11-15). The F401 could not sample CHxN: its only break timer, TIM1, is also its
  only timer whose update reaches DMA2, the one DMA that reaches the GPIO ports (`ea7a9f8`,
  2026-10-31). The STM32F4 Discovery's run D drives TIM1 CH1N with TIM8 pacing the
  sampler; PWM-008 names it and the H753's run J (TIM1 CH3N), whose CHxN duty had been
  counted under PWM-001 only.

- **Errata are data, and `freeze` keeps PRFTEN off on an F405 revision A** (2026-11-15,
  ADR-010). Errata knowledge was spread over 15 cited sections in 11 files, the hand
  verdict table of `h7-errata.md` (ES0392 only, already drifted) and DEBT, with nothing
  tying an erratum to its revisions; `freeze` set PRFTEN on every F405 (ES0182 2.2.1,
  F405 glue `74a8dcd`). Every row of ES0182 Rev 19, ES0299 Rev 6 and ES0392 Rev 15 is now
  an entry with its affected `REV_ID`s and FerroHAL's handling; `freeze` checks the die's
  prefetch erratum; a host test per chip keeps `docs/errata/` and every citation in
  `src/` in step; each bench run's closing lines report the errata of its revision.

- **The glue generator's plan is done, and maturity is computed** (2026-11-15, plan step 7;
  ADR-008, ADR-009). Every part's routing glue is generated and gated, the glue probe
  checks each die on the silicon, and each part's platform and capability tiers come from
  the evidence (`glue-gen tiers`), each run stamped with its bench's tier by reqtool. The
  finished plan was deleted; `docs/design/glue-generator.md` and `maturity.md` hold what
  it decided.

- **The generated glue has a merge gate** (2026-11-15, plan step 6). Step 4 landed the
  generator before its gate, by the plan's order, and `tools/check_glue.py` kept reading
  the rows as text. `tools/glue-gate.sh` regenerates every part's glue and CHIPS.md's
  tables and fails on any difference; CI runs it on pull requests into `main`.
  `check_glue.py` and the `BONDED` tables it read are gone; the glue probe (step 5)
  checks the facts on the silicon instead.

- **A freed UART's receive stream stops interrupting** (2026-11-14). `Uart::free` halted
  the receive stream without clearing its interrupt enables, which the transmit half
  already did (`af65cad`'s receive halt, kept by the `free()` redesign). The STM32F4
  Discovery's run A, the first bench to hand a freed UART's vectors to another UART,
  measured it: after USART3 was freed, DMA1 stream 1 set its transfer-complete flag
  once stopped and stormed (ICSR's active vector 28, IRQ 12), starving the next route.
  The receive halt now turns the stream's interrupts off first; run A passes all 14
  checks.

- **The routing glue is generated** (2026-11-14, plan step 4; API review S1, U1). The
  glue rows had been hand-copied from metapac per chip file since the prototype
  (`af65cad`), the F405's and H743's copied per file (`74a8dcd`). `glue-gen emit` now
  writes each part's routing module from its die table, cut to its device and package;
  the comparison against the hand rows the benches proved found none missing or
  different. A die's hand facts are written once (`shared/die_0x413.rs`, `shared/h7.rs`),
  and `stm32f407vg` was added with no hand routing at all. `docs/CHIPS.md`'s silicon and
  instance tables are generated. What stays open: the merge gate (step 6).

- **A port splits into its bonded pins only** (2026-11-14, owner chose the generated
  shape). `Port::split` handed out all sixteen pins of any port the package bonds at
  all (`af65cad`; W7, `8c7c909`, removed phantom ports, not pins), so an unbonded pin
  (PB11 on the F401RE, PD3 to PD15 on the F405RG) could be named and driven to no
  effect. The generated glue now emits one parts type per bonded port, with a field per
  bonded pin, behind a sealed `gpio::Split`; `gpio::Parts<P>` names it. A no-metapac
  probe holds it: the H743VI's PH2 does not compile (E0609).

- **One instance shape, and every handle frees what it was opened from** (2026-11-13,
  plan step 3; API review D1, D2, D4, U4, D3 in part). Every driver's `Instance` now
  carries `NAME`, `ENABLE` and `KERNEL` as constants and one accessor, `info() ->
  &'static Info`, with private fields, so the timers' `TimCore` no longer crosses the
  API. The kernel clock is one type, `rcc::Kernel` (`Bus`, `Timer`, H7 `Pll1Q`);
  `spi::Kernel`, `i2c::Kernel` and the timer and ADC `BUS` constants are gone. SPI's
  shared traits moved out of its DMA engine into `spi/instance.rs`. The UART, SPI, I2C
  handles and the timer lanes are generic over their wiring, and `free()` returns it
  (free.md rule 0); the four lane-assembly bodies are one. GPIO ports and DMA
  controllers keep crate-internal register supertraits: they are infrastructure a
  driver borrows, not drivers a user opens. Breaking for FerroWasp (owner: allowed,
  the FerroHAL bump already breaks it). Hardware runs pending with PR #44's.

- **The finished plans are deleted** (2026-11-13, owner). The debt sweep and its gates
  (PR #29), the `free()` redesign (PRs #35 to #38), W14 (PR #39) and the documentation
  sweep (PR #30): their facts already lived in the design documents, the decision
  records, the code and this file; the one left, the CI rerun for a gitleaks CDN 504,
  moved to `AGENTS.md`. Their commits keep the record.

- **One DMA-buffer error, and the timers refuse with their own** (2026-11-13; API review
  D6, D7, N11, B3). `uart::ConfigError` had been the crate's buffer error since the
  timer lanes reused it (`ea7a9f8`), and a bad sampler rate refused as the UART's
  `Unsupported`. Now `dma::BufferError { Unreachable, Cacheable }` is what
  `chip::check_dma_buffer` returns, and the UART's and SPI's `ConfigError`, the timer's
  `ConfigError` and the ADC's `Error` each wrap it as `Buffer(_)`. The timer's error
  gains `DmaBusy`, the lane and sampler refusals carry it, and the sampler refuses a bad
  rate with the solver's own reason. F401 run V and H753 run U expect the timer's
  `Buffer(Unreachable)` and `Inexact`; run A's generator follows. Hardware runs pending
  with the PR's.

- **USB's panics are G2's listed exception, not debt** (2026-11-13, ADR-007). The owner
  rewrote G2 as no panic by default, with a feature that can panic allowed when off by
  default and listed in `docs/policies/features.md`. DEBT's "USB sits outside the
  no-panic proof" (`9bd9d1e`, 2026-11-02) is that list's first entry, `usb`; a panic-free
  USB core is on the roadmap (`docs/CHIPS.md`).

- **DEBT holds debt only: tagged, each with its recorded origin** (2026-11-13, owner).
  Of DEBT's 80 limitation items, 29 stay as debt (as 27 entries) and 51 leave it. Moved:
  silicon limits and documented contracts (UART v2 errata, wait floors, ring and
  vector contracts, H7 clock and memory limits) to `docs/CHIPS.md` Limits; features not
  built (frame formats, slave modes, LPUART, BDMA, input capture, ADC triggers, VOS0 and
  the rest) to `docs/CHIPS.md` Not supported yet. Dropped as proven elsewhere under the
  car rule: the flight boards' I2C, UART4/5 on `usart_v2`, motor pads on both smoke
  boards (their IP versions are proven on the Nucleos, their rows glue-checked).
  Dropped as another firmware's: the smoke bench's copy of a user's NOR commands, and a
  user image's DFU `leave` (toaster rule). Added: `Adc`, `AdcScan`, `Sdmmc` and `UsbFs`
  have no `free()`, and RESET-004's missing method. `hil/RIG.md`'s Lucid part fixed in
  place (STM32H743VIH6, TFBGA100). The hardware-tested table moved to
  `hil/COVERAGE.md`.

- **Every library file owes tracing, and CI checks with the reqtool that records**
  (2026-11-13). DEBT's "tracing isn't enforced per file" closed: `reqtool.toml` turns
  on `every_file_traced` and leaves `hil/` out with `every_file_traced_except`, which
  reqtool gained after the item was written. Turning it on changed no requirement's
  status: every library file already declared its IDs. The sweep also found CI pinned
  reqtool at `b28689a`, older than board evidence following files (reqtool#4, #5),
  so CI validated with rules the evidence was not recorded under; the pin is now
  `6b2e748`. The item's other halves are not debt: the diff-analysis record exists
  (`reqtool review`), and the rustdoc requirement pages are a proposal
  (`docs/design/requirements-tooling.md`).
- **A file serving two IP versions has one hash; that is decided, not open**
  (2026-11-13). DEBT's "five source files each serve two IP versions" asked whether a
  module's hash covers shared sources. `docs/design/requirements-system.md` decides
  it: a shared file has one hash, a change re-owes both versions, and under the car
  rule each version needs its own chip. `docs/design/naming.md` lists the files
  compiled under two names; the review's wider count stays open as K4 (DEBT §6).
- **rustdoc is clean on all four chips, and `checks/run.sh` keeps it so** (2026-11-13).
  Nine warnings no ledger held: links to the crate-internal `enter_bootloader`, to a
  `UartIrq::verify` that is `UartInterrupts::verify`, to the H7-only
  `start_windowed` from shared docs, register bit ranges read as links, and `Awg`'s
  docs pointing at its private module (moved onto the type). New step
  `rustdoc (<chip>)` denies warnings. Comments only: no requirement went stale.
- **Two items reclassified as not FerroHAL debt** (2026-11-13). "No requirement covers
  what's listed elsewhere in this file as missing or untested" restated a rule (a
  requirement says only what is measured), not a gap. DEBT §6 "Transition items" held
  only FerroWasp's compatibility adapters, which FerroHAL does not contain; ADR-005
  and FerroWasp track them. §6 now holds the API review's findings.
- **API review findings D5, K2, K3, K5-K8 closed** (2026-11-13). D5 (erasure or tokens):
  the `free()` redesign put the token in every handle (`docs/design/free.md`). K2,
  K3, K5, K6: W14 and #38 (`REVISION-001`, the part-family rule, wrapped trailers read
  by reqtool and `tools/check_names.py`, `UARTDMA-011`/`-012` in the trailer). K7, K8:
  fixed in place (`src/lib.rs` module list and comment, merged doc comments in
  `gpio_v2` and `sdmmc_v2`, ADC `Config`'s doc, a SAFETY note naming the wrong field,
  the plan's stale next-free IDs). The other 54 findings are DEBT §6.

- **`Spi`, `BlockingSpi` and `I2c` hand their tokens back** (2026-11-12). DEBT's
  "`Spi` and `I2c` have no `free()`" closed the `free()` redesign
  ([ADR-002](decisions/2026-11-08-free-teardown-brief.md)): `Spi<U>`,
  `BlockingSpi<U>` and `I2c<U>` carry the instance token; `free()` stops the
  streams (SPI) or closes the unit (I2C, polled SPI) and returns it; a
  constructor refusal hands the token back with the wiring; and `Spi::check`
  reads the instance from the type (`Spi::<SPI1>::check`), as `Uart` already
  did. The bench's six second-use timer checks folded back into runs H and I
  from the freed tokens, so the fresh-boot runs U and T are gone and no token
  is stolen anywhere.
- **`usart_v4` parity has hardware evidence** (2026-11-13). DEBT's "two UART faults
  still have no evidence" kept only `UARTDMA-009`: the H753's links looked all
  loopbacks, but Z6 (PC10 to PC11) reaches two different UARTs, USART3's TX and
  UART4's RX, with their other pins on Z7 and Z5. H753 run Y crosses them: even
  parity into odd reaches UART4's reader as a parity error [UART-014], half the
  baud rate as a framing error, and a line crosses again after both go back to
  8N1. Measured: run Y 5 of 5.
- **An `Awg` frees its timers, and a stop holds the lines low** (2026-11-13).
  DEBT's "an `Awg` has no teardown" closed: `Awg::free` runs `abort` and hands
  both timer tokens back. Its bench check found a worse fault: a stopped timer
  makes no update, so the duty 0 that `abort` and `Pwm::free` wrote never loaded
  and an output stopped mid-pulse stayed high. Both now latch the duties. Measured
  on TIM3 PB0/PB1, F401 run I and H753 run J: after `abort` and after `free`, no
  sample of 1000 high, and the freed token reopens TIM3 at its period [API-002].
- **A split UART's frame can change** (2026-11-11). DEBT's "`set_config` exists only
  on the whole `Uart`" went stale when the token moved into the line:
  `Line::set_config` and `Line::send_break` borrow the transmit half, and
  `Uart::set_config` is that same call on the whole UART, so the benches' frame
  changes run it. Since 2026-11-13 the halves carry their instance, so only the
  line's own transmit half compiles in.
- **FerroWasp adopted FerroHAL; the prototype's open questions are closed** (2026-11-10). DEBT §6 carried the map's two: the kill condition, undecided, and how to measure reuse. The swap landed - FerroWasp's `main` pins FerroHAL `b6d8e84` with `rt`, and the Foxeer's and the Lucid's images run on it - so the kill condition is moot: FerroHAL is the hardware layer, pinned by exact revision until it is published ([ADR-005](decisions/2026-10-31-ferrowasp-integration.md)). Reuse has its standing measurement: the shared UART engine runs unchanged on `usart_v2` and `usart_v4` - the per-IP `Line` is about 150 lines, the engine about 800 - measured as it shipped.
- **`hil/run-dfu.sh` refuses an ambiguous DFU pick and checks the download** (2026-11-10). It flashed whichever DFU device first matched the flash layout - the H743VI and H753ZI both list `16*128Kg` - and ran on after `dfu-util`'s trailing `get_status` error, which also hides a failed download (inferred from the script; found in review). More than one matching device now lists them and stops, `<BOARD>_DFU_SERIAL` pins the pick, and a flash must show `File downloaded successfully`; matches count per device (`devnum`), because one device lists a line per alt setting - a first pin version counted lines and refused the one Lucid it was meant to pick. Measured: a stub listing is refused at three devices on the layout and at two devices sharing one serial, a serial matching one device's two alt settings passes the guard, and a download without the success line fails the flash; the Lucid's bench then passes 51 of 51 with `LUCID_H7_DFU_SERIAL` set. Its H7 prints its IDCODE as the DFU serial (`200364500000`) and the H753ZI's IDCODE measured the same, so the count guard, not the pin, tells that pair apart.
- **Every boot's watchdog checks reach the exit status** (2026-11-10). Runs J and K printed boot 1's fresh-start and debug-freeze checks, but only the last boot set the run's exit status, so a failure there passed the run. `hil/common/watchdog.rs` now stakes the starving boot's failure count in a third whole doubleword of `.uninit` RAM beside its magic-tagged marker - whole doublewords, because the H7's SRAM ECC drops an incomplete one across a reset (RM0433 2.4) - and the next boot's `after_reset` reports it; run K's window marker already carried boot 2. Measured: `run-h753.sh k` passes 10 of 10 and `run-f401.sh j` 7 of 7, both with `the starvation boot passed its checks` (2026-11-10).
- **FerroWasp's H7 bootloader request survives the reset** (2026-11-10). DEBT §5 expected it lost: `ferrowasp-stm32h7-critical` wrote one word to AXI SRAM just before the reset, which RM0433 2.4 says a reset loses. Since the swap, ferro-wasp's `system_bootloader` calls FerroHAL's `reset::reboot_to_bootloader`, which fills the doubleword, and the Lucid entered its ROM DFU from FerroWasp's `reboot dfu` with no BOOT hold (DEBT's H743VI status, 2026-11-10).
- **A windowed watchdog runs, and run K proves both sides of its window** (2026-11-10). `start_windowed` wrote WINR before the prescaler and reload settled, never waited for WVU, and ended with a `0xAAAA` reload. Writing WINR already reloads the counter (RM0433 45.3.3), so that reload came with the counter above the window and reset the chip: measured on the Nucleo-H753ZI as a watchdog reset within 1 ms of `start_windowed` returning, every time. Run K still passed IWDG-005, because the reset it saw was the start's own, and no requirement said a feed inside the window must survive. Now the start waits for the status register to clear, writes the window last and waits for WVU, with no key reload after it; IWDG-007 states the other half, and run K feeds five times inside the window before the early feed, carrying its progress and failures across the resets in a magic-tagged doubleword. 7 of 8 runs passed all 9 checks; the eighth lost its probe session at a reset, with no failed check. With the old driver put back, run K fails IWDG-007. `src/iwdg.rs`, `requirements/benches.md` § Watchdog on the Nucleo-H753ZI.

- **The Lucid's die is revision V, and its UID is read** (2026-11-10). `signature::revision()` reads `DBGMCU_IDC.REV_ID`, and the USB-reporting benches print it beside the UID: the Lucid reports 0x2003, which ES0392 Table 2 maps to revision V, so 2.2.10 (AXI SRAM reads) and 2.2.21 (480 MHz/VOS0) do not apply to it. `hil/run-lucid.sh` passed 51 of 51 with `PASS rev: 0x2003 (V)` and twelve non-zero UID bytes; SIG-002 covers the read, and the earlier `device_uid` item is closed by the same log.

Older crate-wide entries, up to 2026-11-09: [RESOLVED-crate-wide-to-2026-11-09.md](RESOLVED-crate-wide-to-2026-11-09.md).
