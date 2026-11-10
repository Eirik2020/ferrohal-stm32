# Bench Procedures

What each requirement's bench entry runs. The rig itself, what it can prove and what to
improve next, is [hil.md](hil.md). A run proves the files that declare the
requirement it covers, so after the first pass, a change to one file owes only that
file's requirements a new run.

Every UART check prints the requirements it verifies in square brackets after its
name, so a log shows which layer each line speaks for. A record's image hash is the
hash of the manifest beside its logs, which lists every binary of the run by SHA-256.

How the rig is driven, before any procedure here runs — which probe a script flashes
through, and the `rt` feature every firmware crate needs — is [rig.md](rig.md).

## UART on the Nucleo-F401RE

`hil/run-f401.sh a b c d`, with the links in
`hil/nucleo-f401re-loopback/src/board.rs` fitted and the host echo for USART2
running, which the script starts. Pass: every check in every run passes and the
script exits zero. Covers `usart_v2` on all three USARTs, all 10 TX and RX pin
entries and all 9 USART DMA routes, and the checks in `hil/common/tests.rs`. Run A
also masks USART1's line, sends one byte and unmasks it: no chunk ends while the
line is masked, and the idle that arrived meanwhile delivers the byte after
unmasking [USART-005]. Run B
also checks the parity error's clear, each way: one byte with a parity error, then
a good frame straight after it, which must arrive whole and in order, with the one
parity error reported right after the bad byte, and PE seen by the UART's vectors,
counted and reported once. Its log states whether RXNE was set when a vector first
saw PE, and how many bytes the receive DMA had taken by then.

## UART on the Nucleo-H753ZI

`hil/run-h753.sh a b c d e`, with the Zio links in
`hil/nucleo-h753zi-loopback/src/board.rs` fitted. Pass: every check in every run
passes and the script exits zero. Covers `usart_v4` with the caches off and on, and
the same checks in `hil/common/tests.rs`; run A also masks USART1's line for the
USART-005 drill, as the F401's run A does.

Run M (USB CDC over CN13, off the default list while the cable powers the Pico)
builds `run-m` before starting `hil/usb_cdc_host.py`, so the host's wait for a port
is not spent compiling, and the host opens only a port that appeared after the one
a previous image left had gone: a rerun cannot talk to the image it is about to
replace. `FERROHAL_SERIAL_GLOB` overrides the by-id pattern the host matches.

## No reachable panic

`checks/run.sh`'s `no reachable panic` step builds `checks/no-panic` per chip in
release. It calls every public function with optimiser-opaque inputs, and its
panic handler calls an undefined symbol, so the image links only if no panic
path survives. Pass: the step passes for every chip. A new public function is
added to that crate [API-001].

## Chip glue against metapac

`python3 -I tools/check_glue.py`, which compares each chip's routing rows with
stm32-metapac's metadata. Pass: it exits zero. It reads the rows as text, so their
layout must stay as it is.

## SPI and EXTI on the Nucleo-F401RE

`hil/run-f401.sh e f g k`. With the links in its `board.rs` fitted. Pass: every check in every run passes and the script exits zero.

## SPI and EXTI on the Nucleo-H753ZI

`hil/run-h753.sh f g h l`. With the links in its `board.rs` fitted. Pass: every check in every run passes and the script exits zero.

## Timers on the Nucleo-F401RE

`hil/run-f401.sh h i n o p r u`, with J3 and S1 fitted for O and P; run R needs no links. Measures each timer against the cycle counter, sends waveforms over those wires — run O plays one out over J3 and reads it back, then plays with the lane's output off and checks nothing crosses to the pulled-up far end — and drives the SpeedyBee's four motor lanes from one start, every pin read back by the sampler. Run H first sets scale 3 before `freeze`, for it to replace, and then reads back PWR: the voltage scale `freeze` wrote must be scale 2 (VOS bits 15:14 at 0b10) with VOSRDY set. Run U takes each timer's second use from a fresh boot, so no token is stolen: TIM4 at 8 kHz and TIM2 and TIM5 as 1 MHz timebases. Pass: every check in every run passes and the script exits zero.

## Timers on the Nucleo-H753ZI

`hil/run-h753.sh i j p q t`, with Z2 fitted for Q. Measures each timer against the cycle counter, and sends waveforms both ways over that wire. Run J also starts TIM1 CH3N for one pulse: the sampler sees one period at the set duty and the counter stopped itself [PWM-007]. Run T takes each timer's second use from a fresh boot, so no token is stolen: TIM4 at 8 kHz and TIM2 and TIM5 as 1 MHz timebases. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-F401RE

`hil/run-f401.sh j`. Resets the chip on purpose. Also checks that starting the watchdog froze it for the debugger [IWDG-004]. The run starts at the board's measured LSI, 30.0 kHz, and requires the starvation to land within 5% of 50 ms [IWDG-006]. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-H753ZI

`hil/run-h753.sh k`. Resets the chip on purpose, three boots in all. Also checks that starting the watchdog froze it for the debugger [IWDG-004]. After the starvation report it starts a 100 ms watchdog whose last 30 ms are its window, feeds it five times 85 ms after each reload, inside the window, and checks that the chip kept running [IWDG-007]; then it feeds 10 ms after a reload, above the window, and the next boot checks that the feed caused the reset [IWDG-005]. The window phase's progress and the second boot's failures cross the resets in one magic-tagged doubleword, so a reset anywhere in the phase is named and counted in the exit status. probe-rs can lose its session at one of the resets (`SwdApFault`, 1 run in 8 on 2026-11-10); that run exits non-zero with no FAIL line and is rerun. The starvation run starts at the board's measured LSI, 31.7 kHz, and requires the reset within 5% of 50 ms [IWDG-006]. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-F401RE

`hil/run-f401.sh l`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-H753ZI

`hil/run-h753.sh n`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## ADC on the Nucleo-F401RE

`hil/run-f401.sh m`. Drives an analog pin from a GPIO through its link. Pass: every check in every run passes and the script exits zero.

## ADC on the Nucleo-H753ZI

`hil/run-h753.sh o`. Drives an analog pin from a GPIO through its link. Pass: every check in every run passes and the script exits zero.

## The Foxeer F405 V2 board bench

`hil/run-foxeer.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. It sets scale 2 before `freeze`, for it to replace, and its clock checks read back PWR: the voltage scale `freeze` wrote must be scale 1 (PWR_CR bits 15:14 at 0b01, bit 14 being this chip's VOS) with VOSRDY set. Pass: every check in every run passes and the script exits zero.

## The TBS Lucid H7 board bench

`hil/run-lucid.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. Its SD checks need a card in the slot. Pass: every check in every run passes and the script exits zero.

## The TBS Lucid H7 with its slot empty

`hil/run-lucid.sh`, with no card in the slot. Pass: the bench reports the empty slot and every other check passes, and the script exits zero.

## I2C on the Nucleo-F401RE

`hil/run-f401.sh q s`. With I2C1's SCL and SDA (PB8, PB9) pulled up and no
slave fitted, every kind of transfer to a vacant address is answered by
the bus itself with a NACK, which is what run Q proves the driver's error
paths with: each is reported within its bound, the flags are cleared and
the bus put back, so the next one runs. The run also reads the timing
registers back (CR2's FREQ, CCR and TRISE) against the rate the driver
reports, checks the bound with the cycle counter and without it, and the
refusal of a speed out of range, with the wiring handed back and used.
Run S drives the same driver against the Pico 1 simulator
(`hil/pico-i2c-sim`, wired per `hil/WIRING.md`, I2C3 on PA8/PC9): the
ACK'd paths the vacant bus cannot give — 1-, 2- and 32-byte receives, a
write that lands and reads back, a repeated start the slave observes —
a stretch of SCL within the driver's bound and one past it (Timeout,
then Busy while the slave still holds, then recovery), the data-phase
refusal (an address ACK followed by a refused byte), the simulator's own
transaction count cross-checked against the run's, the exact health
accounting over live traffic, and the vacant address still nacking
after it all. The data-phase refusal is the trickiest piece of the map:
the DW NACKs the refused byte and drops it, so no command is ever
delivered and nothing on the bus can report the refusal — the
simulator's warden spends the arm on a three-millisecond timer, in the
gap the suite's pause and throwaway read leave, and the serve task
applies the map's accounting at its next command. Found on the bench:
an I2C pin handed to the peripheral push-pull (not open drain) drives
SDA high through every device's acknowledge bit, and every transfer to
a live slave NACKs — the runs wire both pins with
`into_alternate_open_drain`. Pass: every check in both runs passes and
the script exits zero.

## I2C on the Nucleo-H753ZI

`hil/run-h753.sh r s`. Run R: the same checks as the F401's run Q on
the TIMINGR IP version, on I2C3 over PA8 and PC9, the one pair no other
run links (PB8 and PB9 are the UART link Z4, and a fitted Z4 shorts SCL
to SDA): a vacant address's NACK reported and recovered from, the
timing register read back, the bounds with and without a cycle clock,
and the refusal with the wiring handed back and used. Run S drives I2C4
over PD12 and PD13 against the Pico 1 simulator (`hil/pico-i2c-sim`,
wired per `hil/WIRING.md`) — the one I2C of this chip a bench can wire
(I2C1 and I2C3's pins are taken, and I2C2's PF0 is driven at all times
by the ST-LINK's MCO through the SB45 the HSE clock check needs;
`hil/WIRING.md` has the accounting) — and the one whose kernel clock
is the APB4 path: the run checks it equals the tree's pclk4. On the
wire it covers what the F401's run S does, with one IP difference the
checks carry: the H7's v2 cannot see a held line — its own recovery
clears ISR.BUSY — so a stuck bus surfaces as a bounded Timeout where the
F401 reports Busy before the wire moves, and the first transfer after a
slave's stretch can be the one that resyncs driver and device; the
checks accept both shapes and count each exactly in the health totals.
Pass: every check in both runs passes and the script exits zero. Run M
(the USB device bench) is off the default list while its CN13 cable
powers the Pico: `./run-h753.sh m` still runs it with the cable moved
back.
