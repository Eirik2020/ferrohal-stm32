# HIL rig: capabilities and improvements

The rig turns requirement claims into measurements. [`benches.md`](benches.md) says
how each run is performed; this chapter is the map of the rig itself: what it can
prove, where its blind spots are, and the order to close them in. `DEBT.md` §8
lists fixes waiting for the rig to reach them; when an item here lands, its run
moves to `benches.md` and the deferral comes out.

## The rig

| Part | Hardware | What it carries |
|---|---|---|
| F401 | Nucleo-F401RE, STM32F401RE (LQFP64), ST-LINK/V2-1 | UARTs, SPIs, timers, EXTI, watchdog, bootloader request, ADC, I2C, DShot sampler, a second board's motor lanes |
| H753 | Nucleo-H753ZI, STM32H753ZI (LQFP144), ST-LINK/V3 | `usart_v4`/`spi_v4`/`timer_v3`/`iwdg_v2`, the caches-on runs, USB CDC (run M), the I2C simulator link |
| Foxeer | Foxeer F405 V2, STM32F405RG (LQFP64) | DFU-flashed flight board: USB, crystal, SPI NOR, IMU with EXTI, battery ADC, motor lanes |
| Lucid | TBS Lucid H7, STM32H743VIH6 (TFBGA100) | DFU-flashed flight board: USB, SD card, I2C barometer, motor lanes |
| Pico | Raspberry Pi Pico 1 W, RP2040 | I2C device simulator at `0x2A` for runs S |
| Host | this machine | `hil/run-*.sh`, `probe.sh`, `usb_cdc_host.py`, `vcp_echo.py`, `dfu-util`, `probe-rs`; logs in `hil/logs/` |

Links, jumpers and shared cables are in `hil/WIRING.md` and each board's
`board.rs` header. Two cables are shared by hand: the H753's CN13 with the Pico,
and the Lucid's CN13 with its DFU runs.

## What it can prove today

- **UART**, all three IP paths: every TX/RX pin entry and DMA route the reference
  manuals list on both Nucleos, loopback and host echo, receive-only links,
  inversion (SBUS), parity errors and their clear, idle-line chunking, line
  masking (USART-005), 64 KiB echo at 561 KiB/s.
- **SPI**: DMA and polled masters, all four modes with SCK idling at CPOL, SCK
  edges counted by EXTI, abort then recovery, the Foxeer's real NOR, SPI1–SPI5
  on the H753's Zio pins.
- **EXTI**: both edges, masks, software trigger, the H7's line muxing.
- **Timers and output**: `Tick`/`Timebase` accuracy against DWT, PWM rates and
  duty, compare lanes and samplers paced by timer DMA (DShot150/300/600),
  one-pulse, lead/follow triggers, the four-lane SpeedyBee arrangement on the
  F401, TIM1 CH3N on the H753.
- **Watchdog**: freeze under a halted core, windowed resets, LSI-scaled timeouts,
  reset cause, the second boot after a reset.
- **Bootloader and DFU**: the request across resets with caches on and off, safe
  entry to the ROM, VTOR after a DFU `leave` (the F405 control run), and DFU
  flashing of both flight boards.
- **ADC**: rails and VREFINT against factory calibration, die temperature,
  sequence over DMA, FerroWasp's battery sequence.
- **I2C**: master paths against the Pico simulator — transfers, a repeated
  start, stretching within and past the bound, refusal, stuck-bus recovery,
  health counts; the Lucid's barometer was proven on this path.
- **USB CDC**: enumeration and 64 KiB echo (run M, rerun-safe), the RTIC-vector
  variant on the flight boards, the VBUS diagnostics that tell a charge-only
  cable from a dead port.
- **Storage**: the Lucid's SD card, one and sixteen blocks; the Foxeer's NOR
  erase/program/read in FerroWasp's scratch sector.
- **Caches, one shape**: the H753 UART runs with I- and D-cache on and the DMA
  window in a non-cacheable MPU region; `check_dma_buffer` refusals of cached RAM
  and the DTCM are checked there.
- **Host-only proofs**: the no-panic link per chip, glue against metapac,
  feature guards, doc budgets, host unit tests. These need no board and are not
  counted as hardware evidence.

## What it cannot prove yet

Grouped by why; each is a candidate for the queue below.

- **Silicon revision**: nothing reads `DBGMCU_IDC.REV_ID`, so revision-dependent
  errata (ES0392 2.2.10 AXI SRAM reads, 2.2.21 480 MHz) cannot be decided per part.
- **Bondout and package facts**: one package per die runs here, and that is
  enough for digital behaviour — the same RTL sits behind every bondout, so a
  second package would add no firmware evidence. What a package does change is
  read, not run: which pins are bonded (UART4/5 below), and package-scoped facts
  such as the TFBGA100 having no VDDLDO supply or VREF+ pin.
- **Cache and MPU shapes**: one configuration (off, or on with one non-cacheable
  window). Write-through memory, MPU-off-with-caches-on, invalidate-then-store
  and DEBR locking are not exercised, so the ES0392 cache rules are inspection.
- **SysTick clock source**: every firmware uses rtic-monotonics' `Core` source;
  the HCLK/8 (`External`) path never runs.
- **WWDG, flash ECC, option bytes/RDP**: no driver and no run; their errata are
  firmware rules only.
- **Constructor refusals**: no run refuses a constructor on hardware and then
  reuses the parts (`DEBT.md` §8).
- **UART4/5 on the F405**: not wired, so the reserved-CTS-bit fix is inspection.
- **`device_uid`**: never read on hardware.
- **USB beyond CDC**: no OTG_HS, no host mode, no VBUS/data fault injection.
- **Error paths**: DMA transfer errors, USART overrun, noise, stop timeout.
- **Electrical**: no rail margining, no current measurement, no scope on edges;
  timing claims are read back through the chip, not measured on the wire.
- **Environment and endurance**: room temperature only, no soak, no automated
  power cycling.
- **Absent interfaces**: RF, GPS, CAN, Ethernet, real ESCs with props on.
- **DFU convenience**: entry needs a running FerroWasp CLI (`reboot dfu`) or a
  hand on BOOT; `run-dfu.sh`'s own request only works from a FerroHAL image.

## The improvement queue

In order; each names what it unlocks. Cheap ones first, then the ones that
unblock a claim.

1. **Record `REV_ID` and the fitted part marking in every run's log.** One check
   plus board constants. Unlocks the 2.2.10 (AXI SRAM) and 2.2.21 (480 MHz)
   decisions and makes the board identity auditable.
2. **A refusal run per chip.** Refuse each public constructor on hardware, then
   use the returned parts. Closes `DEBT.md` §8's largest item and exercises
   error paths for real.
3. **Fault injection.** DMA transfer error, USART overrun, noise, stop timeout —
   the error paths `DEBT.md` names as never exercised.
4. **A cache/MPU matrix.** Write-through region, MPU off with caches on, an
   invalidate followed by a store with and without a DMB. Proves the ES0392
   firmware rules and answers "is it safe for FerroWasp to enable caches?" with
   a run instead of an argument.
5. **UART4/5 on the F405** (or move that check to the H753): closes an
   inspection-only fix.
6. **DFU without a hand on BOOT.** Keep a DFU requester on the host, or a boot
   image that always accepts `reboot dfu`.
7. **Analog margin.** VDDA/VREFINT at their limits and a current shunt where a
   flight claim depends on it.
8. **Soak runs.** Hours-long per board with the watchdog and USB live, to catch
   the drift and lock-up classes a 300 s run cannot.
9. **Environmental corners.** Temperature and supply extremes only where a
   flight-relevant claim needs them.
10. **Wire-level integrity.** A scope on SCK, SDA and DShot edges when timing
    claims get tight enough that read-back is not enough.

## Using it

- A run proves the files that declare the requirement it covers; `benches.md`
  lists the runs and their pass criteria. Logs land in `hil/logs/` and the
  manifest beside each log hashes every binary.
- A claim in a commit or requirement names the run that proves it. A claim based
  on inspection says so and points at `DEBT.md` §8 or `docs/design/h7-errata.md`.
- When the rig gains a capability, add its run to `benches.md`, move the entry
  out of the queue here, and remove the matching deferral from `DEBT.md` §8.
