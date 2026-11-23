# Rig improvements: what each upgrade would unblock

The options for growing the rig, each tied to the open items it would close. The
agreed order lives in [CAPABILITIES.md](CAPABILITIES.md#the-improvement-queue); this
chapter is the wider list behind it, from `docs/DEBT.md`,
[COVERAGE.md](COVERAGE.md), [RIG.md](RIG.md) and [WIRING.md](WIRING.md). Nothing here is
approved: rig upgrades are held by the owner, and an item enters the queue
only when the owner picks it.

Every suggestion keeps two kinds of statement apart. **Measured** is something a run, a
probe read or the owner has recorded. **Inferred** is reasoning from a manual, a
schematic or the source that nobody has checked on this rig; it is a reason to look
closer, not evidence.

## The rules an upgrade must fit

- **One permanent, hands-free set-up**. A procedure that needs a
  jumper moved, a cable swapped or a button pressed by hand is not an answer; a switch
  the rig drives itself is.
- **No soldering for now, allowed later**, headers included. The
  suggestions that need it say so.
- **The car rule** (`docs/design/requirements-system.md`): one chip proves an IP version
  for every chip that carries it, one chip a family's glue. A second board of a die
  already on the rig adds nothing unless it reaches something the first cannot.
- **FerroHAL's rig only.** FerroWasp's hardware needs are not folded in (the toaster rule, `docs/design/scope.md`).
- **On order** (arrival not known): an Adafruit 4682 microSD
  breakout, two CAN transceivers, an SPI NOR flash breakout and an external DAC.

## What the rig cannot reach today

The items that wait on hardware, from `docs/DEBT.md` as of 2026-11-17 and the gaps in
[COVERAGE.md](COVERAGE.md). The last column names the suggestions below that address each.

| item | where | why the rig cannot reach it | suggestions |
|---|---|---|---|
| No ES0392 2.2.10 workaround (AXI SRAM reads, rev Y/W) | DEBT §1, Medium | both H7s are rev V (0x2003, measured) | D1 |
| H7 I2C lacks ES0392 2.19.5 (reads over 255 bytes, rev Y/W) | DEBT §1, Medium | rev V only, as above | D1 |
| `usart_v2` places a lone parity error late, or loses it, above 1 Mbaud | DEBT §1, Medium | the cause is inside the chip; nothing on the rig sees the wire or the vector's timing | B1 |
| F4 I2C lacks the misplaced-Stop fix (ES0299 2.9.3, ES0182 2.10.3) | DEBT §1, Medium | the Pico is a slave only; no second controller on the bus | A4 |
| The H7 USB port's zero-length packet (ES0392 2.25.7) | DEBT §1, Low | CDC passes; nothing looks at the packets | A5, B4 |
| SDMMC's `free()` only on the Lucid's flashed run | DEBT §3, Low | the Lucid is the only board with a card slot | C1, D2 |
| FIFO error handling unconfirmed | DEBT §3, Low | neither FIFO error has been provoked | A1, B1 |
| A Foxeer smoke image that fails before USB says nothing | DEBT §3, Low | no debug probe on the flight boards | C5 |
| The H753 bench reaches the Zio headers only | DEBT §4, Medium | no morpho headers fitted; UART5's PB13 needs SB7 | C2 |
| SDMMC's error paths have no bench | DEBT §4, Low | no slot on a Nucleo, no way to pull a card or corrupt a line | C1 |
| HSE crystal mode untested on both Nucleos | DEBT §4, Low | both run HSE bypass from the ST-LINK's MCO | C4, D2 |
| F401 USART2 runs through the ST-LINK | DEBT §4, Low | the ST-LINK's VCP sits on PA2/PA3 | A6, C3 |
| The F401's clock tree checked only indirectly | DEBT §4, Low | the Nucleo-F401RE has no USB device to time against | B1 |
| HCLK/8 SysTick, WWDG, option bytes, flash ECC, another revision | DEBT §4, Lowest | partly firmware work, partly silicon the rig lacks | A2, A3, D1, D2 |
| Analog margin, soak, environment, wire-level timing | CAPABILITIES queue 2 to 5 | no instruments | B1, B2, B3, E1 |

Measured as of 2026-11-17: every requirement in `src/requirements/` names a
verification method, and DEBT's status line records reqtool at 128 of 128 verified.

## A. No new hardware

Some items read like rig work but need only firmware or a host tool. They come first
because they cost nothing and keep the shopping list honest.

- **A1. Provoke the DMA FIFO errors in firmware first** (DEBT §3, FIFO error handling).
  Inferred from RM0090/RM0433: in direct mode a FIFO error means the stream could not
  keep up with its peripheral, so a high-priority memory-to-memory stream on the same
  controller, run against a UART at its top baud, may starve it on purpose. If it
  cannot be made to fail, B1's stimulus is the fallback.
- **A2. The HCLK/8 SysTick path and WWDG** (DEBT §4, Lowest). Inferred: neither needs
  hardware the rig lacks; the HCLK/8 path is a bench image that selects `External`, and
  WWDG needs a driver first, which is roadmap (`docs/CHIPS.md`), not rig.
- **A3. Flash ECC on the H7** (DEBT §4, Lowest). Inferred from RM0433: writing one
  flash word twice is the usual way to provoke an ECC error, so it needs firmware, not
  a rig change; it wears a sector, so it belongs on a sacrificial board (D2). Option
  bytes and RDP likewise: RDP level 1 back to 0 erases the flash, and level 2 cannot be
  undone, so neither goes near a board the rig depends on.
- **A4. Use the Pico as a second I2C controller** (DEBT §1, the F4 misplaced-Stop
  erratum; the H7 and F4 BERR items). Measured: the Pico shares both buses (WIRING.md).
  Inferred: its PIO can drive SDA and SCL directly to place a Start or Stop where the
  sheet says, with no new wire. A spurious BERR, as the sheets describe it, cannot be
  provoked on demand; a real one can, which proves only that the driver recovers.
- **A5. Capture USB on the host before buying anything** (DEBT §1, ES0392 2.25.7).
  Inferred: the host's `usbmon` shows what the controller delivered; a malformed packet
  may be dropped below it, so silence there is not proof (B4 is the answer if it matters).
- **A6. Ask whether F401 USART2 needs its own link at all** (DEBT §4). Measured: USART1
  and USART6 share `usart_v2` with USART2 and are proven on their own links (COVERAGE,
  runs A to D), and run B checks parity on the F401 (DEBT §1's parity item). Inferred:
  under the car rule the IP's break, baud and parity could stand on them, leaving only
  USART2's pin rows, which the glue probe already checks; whether the runs cover break
  and a baud change on those links is not checked here. An owner
  decision, not a rig change; C3 is the hardware route if the answer is no.

## B. Instruments

- **B1. An instrument Pico: logic analyser and stimulus.** A second Pico 1, wired
  permanently to free pins on both Nucleos, on its own USB cable. Unblocks:
  - the `usart_v2` parity item (DEBT §1): it can capture the RX line beside a GPIO the
    vector toggles, showing whether PE is late or never rises, and it can send a lone
    bad-parity byte from an independent clock, taking the F401's own transmitter out of
    the question;
  - the F401 clock item (DEBT §4): it can count a timer output against its own crystal,
    an independent reference the F401 has none of today;
  - FIFO errors (DEBT §3), if A1 fails: a sustained stream at a rate the chip's own
    transmitter cannot give;
  - queue 5 (wire-level integrity), in part: SCK, SDA and DShot edges at its sample rate.

  Measured: the I2C simulator Pico uses GP0 to GP3 and its USB port; the rig has spare
  micro-USB cables (the 2026-11-14 delivery). Measured: both MCO pins of the F401 (PA8,
  PC9) carry the I2C3 link to the simulator (WIRING.md), so a clock check uses a timer
  output instead. Inferred: a Pico samples fast enough for 5.25 Mbaud on a few channels,
  not for SPI at its top rates, and its crystal's tolerance must be looked up before a
  ppm claim rests on it.
- **B2. A USB hub with per-port power switching** (`uhubctl`). Unblocks the "no automated
  power cycling" gap and queue 3 (soak runs), and gives a cold boot, not just a reset,
  hands-free. Inferred: it cannot press a flight board's BOOT button, so it does not
  make DFU entry hands-free on its own (C5 does).
- **B3. A current and voltage monitor, with the ordered DAC.** An INA226-class shunt
  monitor in a board's supply, read over I2C, and the DAC driving an ADC input with known
  voltages. Unblocks queue 2 (analog margin): ADC readings against an independent
  source rather than VREFINT alone. Inferred: no DEBT item needs it today; it earns its
  place when an ADC accuracy claim does.
- **B4. A USB protocol analyser.** Only if A5 leaves ES0392 2.25.7 open. Inferred: a
  hardware analyser sees the packet the device sent, which is what the erratum is about.

## C. Breakouts and wiring

- **C1. The ordered microSD breakout on the H753's SDMMC1, through an analog switch.**
  Unblocks queue 1 and SDMMC-001 to 004 at `HIL` on a Nucleo; SDMMC's `free()` (DEBT §3)
  on a bench API-002 can name; and, with the switch, SDMMC's error paths (DEBT §4).
  - Measured: SDMMC1's pins (PC8 to PC12, PD2) are on Zio CN8, and PC10/PC11 and
    PC12/PD2 are already the Z6 and Z8 UART links (`board.rs`; CAPABILITIES queue 1).
    FerroHAL drives SDMMC1 only (`src/sdmmc_v2/mod.rs`), and SDMMC2's alternate pins
    collide with S1, S2, Z1 and Z12 in the same way.
  - So under the hands-free rule the card cannot simply replace Z6 and Z8. Two ways out,
    the owner's choice: drop Z6 and Z8 (UART5 then has no Zio route, and UART4's PC10
    entry goes), or put a multi-channel analog switch, driven by a spare GPIO, between
    CN8 and both the card and the UART links. Inferred: the same switch, plus a load
    switch on the card's supply and one on a data line, can pull the card mid-transfer
    and force a CRC failure, the two error paths DEBT §4 names. Its on-resistance and
    capacitance have to suit SDMMC's clock; check before choosing a part.
  - Measured: the breakout must carry CMD and DAT0 to DAT3 both
    ways; an SPI-only one with a one-way level shifter does not fit.
- **C2. Morpho headers on the Nucleo-H753ZI, and SB7.** Needs soldering (later). Unblocks
  DEBT §4's Zio-only item: UART8 TX, PA8, PA9, PB14, PG9 and the 22 TX/RX entries now
  glue-checked only, and UART5's PB13. Measured: the board has no morpho headers fitted
  (WIRING.md). Inferred: with headers, C1's conflict could be solved by moving Z6 and Z8
  to morpho-only pins instead of switching them.
- **C3. A direct USART2 link on the F401** (SB13/SB14 off, SB62/SB63 on). Needs
  soldering. Unblocks DEBT §4's USART2 item, unless A6 settles it. Measured: the host
  echo in `hil/run-f401.sh` runs over the ST-LINK's VCP on that USART (RIG.md). Inferred:
  the bridges take the VCP away, so the echo would need a USB-serial adapter of its own.
- **C4. A crystal on a Nucleo.** Needs soldering (the crystal, its capacitors, and the
  solder bridges that today feed the MCO in). Unblocks DEBT §4's HSE crystal item for
  that family. Measured: the Discovery cannot isolate crystal mode without rework (R68 and R25), and the flight boards' crystals are proven only at `FLSH`
  (COVERAGE). D2 is the route without a soldering iron.
- **C5. SWD on the flight boards.** Wires to the Foxeer's and the Lucid's SWD pads, to a
  spare probe. Needs soldering. Unblocks DEBT §3 (a Foxeer image that fails before USB),
  the hands-free DFU entry CAPABILITIES lists as missing, and lifts their runs from
  `FLSH` towards `HIL` (`docs/design/maturity.md`). Inferred: whether each board exposes
  SWD pads has not been checked.

## D. More boards

- **D1. An H743 or H753 of revision Y or W.** ES0392 gives both REV_ID 0x1003
  (`docs/errata/ES0392.md`). Unblocks DEBT §1's 2.2.10 and 2.19.5 items, DEBT §4's
  "another silicon revision", and lets the revision-Y limits in `docs/CHIPS.md` (the
  836 MHz VCO, CRS trimming) be checked rather than inherited. Measured: both H7s here
  are V (RIG.md). Inferred: early Nucleo-H743ZI boards (not the -ZI2) are the likeliest
  source, bought used; the revision has to be read with `hil/rig_status.py` on arrival,
  never assumed from the box. Under the car rule one such board covers every Y/W rule.
- **D2. A crystal-fitted board with its own USB and SWD, used as a sacrificial board.**
  Something like a WeAct-style STM32F4 or STM32H743 module. Unblocks DEBT §4's HSE crystal
  item without solder; an H743 module with a card slot is a second route to C1 with no
  CN8 conflict; and it is the board the option-byte, RDP and flash-ECC runs (A3) can
  wear out. Inferred throughout: check each module's schematic for the crystal, the
  slot's wiring and the SWD header, and its die against the chips FerroHAL supports
  (the smaller F401 parts are not die 0x433) before buying. An F411 module would also
  serve the F411 roadmap once FerroHAL supports that die.
- **D3. An F405 of revision A.** It would confirm DEBT §1's REV_ID 0x2000 item on
  silicon. Inferred: revision-A parts are early production and unlikely to be found;
  the item's host test is the realistic proof.
- **Already attached, nothing to add yet.** Measured: a Nucleo-F767ZI (die 0x451) and a
  Nucleo-G474RE (die 0x469) are on the rig with their probes. They unblock nothing until
  FerroHAL supports their families; the car rule makes wiring them premature.
- **Ordered parts with no DEBT item.** Inferred: the CAN transceivers serve the roadmap
  (no CAN driver yet), and the SPI NOR breakout would give `spi_v2`/`spi_v4` a real
  device at `HIL` on a Nucleo, where the Foxeer's NOR is `FLSH` only; neither closes
  an open item today.

## E. Environment

- **E1. Temperature and supply corners** (queue 4). A small heated enclosure or a
  bench supply for the board under test. Inferred: no DEBT item needs it today; take it
  up when a flight-relevant claim does, as the queue says.

## What no upgrade here closes

- Package facts, such as which pins a bondout carries: read from the datasheet, not run
  (CAPABILITIES).
- RF, GPS, Ethernet, and ESCs with propellers on: outside FerroHAL's scope or roadmap
  only.

## Summary

| id | suggestion | unblocks | solder | state |
|---|---|---|---|---|
| A1 to A6 | firmware, host tools, an owner decision | FIFO, SysTick, ECC, I2C errata, USB ZLP, USART2 | no | nothing to buy |
| B1 | instrument Pico | parity above 1 Mbaud, F401 clock, wire timing | no | needs a Pico |
| B2 | switched USB hub | soak, cold boot, power cycling | no | needs a hub |
| B3 | shunt monitor and DAC | analog margin | no | DAC on order |
| B4 | USB analyser | ES0392 2.25.7 | no | only if A5 fails |
| C1 | microSD on SDMMC1 via a switch | SDMMC at `HIL`, error paths, `free()` | no | breakout on order; switch needed |
| C2 | morpho headers, SB7 | the Zio-only rows | yes | later |
| C3 | F401 USART2 direct | USART2's link | yes | only if A6 says so |
| C4 | crystal on a Nucleo | HSE crystal mode | yes | later, or D2 |
| C5 | SWD on flight boards | silent Foxeer failures, DFU entry, tiers | yes | pads unchecked |
| D1 | rev Y/W H7 | 2.2.10, 2.19.5, another revision | no | needs sourcing |
| D2 | crystal module, sacrificial | HSE crystal, SD route, RDP/ECC runs | no | needs choosing |
