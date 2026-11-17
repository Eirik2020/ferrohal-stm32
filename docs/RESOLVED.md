# FerroHAL: resolved debt

History: items moved out of [DEBT.md](DEBT.md) once done. The chip and feature sections continue in [RESOLVED-chips-and-features.md](RESOLVED-chips-and-features.md); crate-wide entries from 2026-11-10 to 2026-11-16 in [RESOLVED-crate-wide-2026-11-10-to-16.md](RESOLVED-crate-wide-2026-11-10-to-16.md), and up to 2026-11-09 in [RESOLVED-crate-wide-to-2026-11-09.md](RESOLVED-crate-wide-to-2026-11-09.md). Limits a fix left behind stay in DEBT.md as open items. Newest first within each section. Section numbers (§) refer to the architecture map, dissolved into `docs/design/` topics on 2026-11-10; the numbers cite it as it was.

## Crate-wide

- **API review D3: the engine-backed IP modules are crate-private** (2026-11-17).
  `usart_v2`/`v4`, `spi_v2`/`v4`, `i2c_v1`/`v2` and `adc_v2`/`v4` were public beside the
  neutral `uart`, `spi`, `i2c` and `adc` that board code uses, and exposed metapac
  register types (`Regs`). They are `pub(crate)`: each is its IP version's register
  wrapper (`Unit`, `LineRegs`) behind the neutral driver, which re-exports everything a
  board names. Modules that are the driver themselves, under a neutral alias (`gpio_v2`
  as `gpio`, `rcc_f4` as `rcc`, `exti_*`, `iwdg_*`, `timer_*`, `dma_*`, `sdmmc_v2`),
  keep their public versioned names, as ADR-003 rule 4 records. FerroWasp's
  `uart_dma.rs` imports `usart_v2::{Instance, ...}`; `uart::` re-exports the same at
  its next bump. Spawned by each driver's own re-exports.

- **API review U6: SPI's two engines share their clock side; one `Info` shape**
  (2026-11-17). The DMA and polled SPI each kept the kernel clock, the SCK in use, the
  CPU cycles per SCK and the ES0392 2.22.6 hold after the end, and computed them twice.
  `spi::instance::SckClock` holds them once (`configure`, `reconfigure`,
  `hold_after_end`). `spi::Info` and `i2c::Info` were the same type written twice; both
  are now `resource::Info<Unit>`, each module stating `Sync` for its own `Unit`. Spawned
  by `66afdaa` copying `99e6404`; `cdfe190`.

- **API review D8: refusals carry `ConfigError`** (2026-11-17). UART, SPI, I2C and the
  timers refused with a `ConfigError`; the ADC, IWDG and SDMMC with their runtime
  `Error`, the ADC's mixing `KernelClock` and `Sequence` with `Overrun`. Now
  `adc::ConfigError` (`KernelClock`, `Timeout`, `Sequence`, `DmaBusy`, `Buffer`) is what
  `Adc::new` and `into_scan` refuse with, and `adc::Error` keeps the runtime faults;
  the IWDG's errors, all refusals, are `iwdg::ConfigError`; `sdmmc::ConfigError::Clock`
  is `Sdmmc::new`'s. USB passes on `rcc::ClockError`, its only refusal.
  `design/api-surface.md` records the rule. Spawned by `af65cad`, `1e1bbdf`, `a8a3a05`,
  `4237048`.

- **API review S5 and N8: test names and routing names** (2026-11-17). S5: the clock
  solver's tests were named by board (`solve_matches_the_foxeer_tree`,
  `_the_lucid_tree`, `_the_proven_tree`, `foxeer_clocks_validate`); they are named by
  the tree they pin, and `design/naming.md` records where such tests live (the chip file
  whose limits they need) and how they are named. N8: the timer's routing traits and
  wirings (`CompareDma`, `UpdateDma`, `LaneWiring`) already follow N2's rule, and USB's
  missing wiring came with D9. Spawned by `8ad4510`; `ea7a9f8`, `9bd9d1e`.

- **API review N18 and N19: one accessor set, one `Config` shape** (2026-11-17). `Spi`
  had no `name` or `kernel_clock`, `BlockingSpi` no `kernel_clock`, I2C no module
  `check::<U>`, `adc::Config` no `new`, and `i2c::Config` hand-wrote its
  `defmt::Format`. Now every bus driver answers `name`, `frequency`, `kernel_clock` and
  `set_clock`; `i2c::check::<U>` and `i2c::Config::check` return the SCL in use, as SPI's
  return SCK; `adc::Config::new` exists; `i2c::Config` derives `Format` (SPI's stays
  written out: embedded-hal's `Mode` lacks it). `design/api-surface.md` records the set,
  including why the polled SPI has no `health`. The UART's follow its redesign. Spawned
  per driver.

- **API review S4: `RESOURCES` beside its wiring** (2026-11-17). I2C, ADC, SDMMC and
  USB defined `Wiring::RESOURCES` beside the struct; SPI's two sat in its engines
  (`spi/stream_dma.rs`, `spi/polled.rs`). Both now sit in `spi/mod.rs` beside `Wiring`
  and `BlockingWiring`, and `design/api-surface.md` records the placement. The UART's
  move with its single-owner redesign. Spawned by `2e7f7fe` against `cdfe190`.

- **API review U5: pin rows from one helper** (2026-11-17). Every wiring's
  `RESOURCES` wrote `Resource::Pin { signal, port: P::PORT, pin: P::PIN, af: P::AF }`
  by hand, 21 times by now (UART, SPI stream and polled, I2C, timer lanes, SDMMC, USB).
  The crate-internal `const fn Resource::pin::<P>(signal)` builds the row from the pin's
  type; the chip files' host tests, which compare the tables row by row, pass
  unchanged. Spawned by `2e7f7fe`'s descriptors.

- **API review U11: one USB smoke bench** (2026-11-17). The Foxeer's and the Lucid's
  `usb_irq.rs` were one file copied, by then differing in their endpoint memory, board
  name, watchdog field and whether they write a stop record. `hil/common/usb_irq.rs`
  holds the RTIC app once, as `usb_irq_app!(board, iwdg, record)`; each board's
  `usb_irq.rs` sets `EP_WORDS` and expands it. Spawned by `ed48419`.

- **API review N13: one `ClockError` for both families** (2026-11-17). The F4 named
  `Sysclk`, `Pclk1`, `Pclk2`, `Usb`; the H7 `Cpu`, `Pclk`, `UsbTimeout`. `rcc_shared`
  now holds one enum, re-exported as `crate::rcc::ClockError`: the H7's `Cpu` is
  `Sysclk`, its single `Pclk` is split into `Pclk1` to `Pclk4` so a refusal names the
  bus, and `Usb` (no 48 MHz source configured) and `UsbTimeout` (HSI48 or the USB supply
  not ready) are both kept, since they mean different things. Spawned by `af65cad`, the
  two RCC modules written separately.

- **API review N5 and N17: "handle" has one meaning; the watchdog's names**
  (2026-11-17; owner's pick A). `design/naming.md` defines a handle as an ownerless,
  freely copied view a second context holds. `TimebaseReader` is `TimebaseHandle`, made
  by `Timebase::handle`; `ExtiHandle` and `Watchdog::handle` already fit. N17:
  `Watchdog::start` stays and is recorded as deliberate (an IWDG cannot be stopped);
  the token `chip::WatchdogPeripheral` is `chip::IWDG` on every chip (the H7's IWDG1).
  The DMA's `FlagHandle` and the UART's `UartIrq` follow with the UART's single-owner
  redesign. Spawned by `b55c640`, `1e1bbdf`.

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
  The next run that reaches USB reports it as a `FAIL` and clears it. Reading it over DFU
  was dropped: the F4 ROM DFU cannot read SRAM ("Page at 0x2001ffe0 is not readable";
  alt 0 maps internal flash only), measured on the Foxeer in #49. That the record lives
  through the ROM is `run-foxeer.sh`'s round trip to show: `BENCHSTP` leaves a probe
  record, the image is flashed over the ROM DFU, and `STOPREC?` must answer with it. The
  Lucid keeps no record yet (`STOP_RECORD` = `None`). Spawned by `926b210` (2026-11-02).

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
  cannot start until SWRST; `i2c_v1`'s `recover` resets through SWRST and writes CR2's
  FREQ, CCR and TRISE back once the bus is idle, and cycles PE, as before, when it is
  not. Measured on F401 run S (2026-11-17): a SWRST on every recovery, the first
  version, left the bus with SDA held low by the Pico after the stretch-past-the-bound
  check, and every later transfer `Busy`; with SWRST only on an idle bus runs Q, R, S and
  V pass, the NACK recoveries taking the SWRST path. Spawned by the ES0299 and ES0182
  row reviews (2026-11-17).

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
