# Bench Procedures

What each requirement's bench entry runs. The rig itself, what it can prove and what to
improve next, is [hil/CAPABILITIES.md](../hil/CAPABILITIES.md). A run proves the files that declare the
requirement it covers, so after the first pass, a change to one file owes only that
file's requirements a new run.

Every UART check prints the requirements it verifies in square brackets after its
name, so a log shows which layer each line speaks for. A record's image hash is the
hash of the manifest beside its logs, which lists every binary of the run by SHA-256.

How the rig is driven, before any procedure here runs — which probe a script flashes
through, and the `rt` feature every firmware crate needs — is [hil/RIG.md](../hil/RIG.md).
The injected faults, split out to keep this file in its budget, are
[benches-faults.md](benches-faults.md); the host analyses (no reachable panic, the
chip glue) are [benches-analysis.md](benches-analysis.md).

A run letter is its bench's alias for a binary named by its test:
`hil/<crate>/runs.tsv` maps them (H753 run M is `usb-cdc`, F401 run V is `refusals`).

## UART on the Nucleo-F401RE

`hil/run-f401.sh a b c d`, with the links in
`hil/nucleo-f401re-loopback/src/board.rs` fitted and the host echo for USART2
running, which the script starts. Pass: every check in every run passes and the
script exits zero. Covers `usart_v2` on all three USARTs, all 10 TX and RX pin
entries and all 9 USART DMA routes, and the checks in `hil/common/tests.rs`. Run A
also masks USART1's line, sends one byte and unmasks it: no chunk ends while the
line is masked, and the idle that arrived meanwhile delivers the byte after
unmasking [UART-020]. Run B
also checks the parity error's clear, each way: one byte with a parity error, then
a good frame straight after it, which must arrive whole and in order, with the one
parity error reported right after the bad byte, and PE seen by the UART's vectors,
counted and reported once. Its log states whether RXNE was set when a vector first
saw PE, and how many bytes the receive DMA had taken by then. Run B also puts USART6 in
half-duplex [UART-021]: a frame it sends from PC6 must reach USART1 and come back
into USART6's own ring, though USART1 sends nothing meanwhile, and a frame USART1
sends into PC7, the RX pin half-duplex leaves unused, must not arrive.
Every link of runs A and C also queues frames until `try_send` refuses one, at most
`TX_FRAMES` plus the one already gone, and each must come back from `take_outcome`
once, in order, as `Sent`, with nothing after [UART-DMA-013 UART-DMA-014]; then sends one
16-byte frame with `set_tx_idle_interrupt` on and calls nothing on the transmit side:
`tx_drained` must turn true by
the USART's interrupt, and on a jumpered link only once every byte has come back
[UART-023]. The log states how many bytes the receiver lacked when the transmit DMA
finished.

## UART on the Nucleo-H753ZI

`hil/run-h753.sh a b c d e y`, with the Zio links in
`hil/nucleo-h753zi-loopback/src/board.rs` fitted. Pass: every check in every run
passes and the script exits zero. Covers `usart_v4` with the caches off and on, and
the same checks in `hil/common/tests.rs`; run A also masks USART1's line for the
UART-020 drill, as the F401's run A does. Run Y crosses two UARTs over Z6, USART3's
TX on PC10 into UART4's RX on PC11, for what a loop cannot show: even parity into
odd reaches UART4's reader as a parity error [UART-014], half the baud rate as a
framing error. Half-duplex [UART-021]: run Y's pair is wired one way, so USART3
in half-duplex must put a frame on PC10 that reaches UART4 and comes back into its
own ring, and back in full duplex its ring (on the unwired PB11) must stay silent.
TX and RX swap [USART-V4-002]: run B runs USART3's PC10/PC11 loop
swapped, which must still carry a line and a stream, and run Y swaps USART3 alone,
so it sends on PB11 and UART4 on PC11 must hear nothing. The outcome and quiet-line checks run in
every suite as on the F401; on `usart_v4` they prove UART-023 [UART-023].

## USB on the Nucleo-H753ZI

`hil/run-h753.sh m`, with the CN13 cable on the H753, as the rig keeps it
(`hil/WIRING.md`). Pass: every check passes and the script exits zero. Before the echo, the port is freed to its token and both pins and reopened from them, so the echo runs on the reopened port [API-002]. The run clocks from HSE in bypass (the ST-LINK-V3's MCO); after the echo it counts CPU cycles over 1000 of the host's start-of-frames and requires 400 MHz within 1 %, past the MCO's own error, measured up to 0.65 % low (`hil/RIG.md`) [RCC-001].

Run M (USB CDC over CN13) builds its binary (`usb-cdc`) before starting `hil/usb_cdc_host.py`, so the host's wait for a port
is not spent compiling, and the host opens only a port that appeared after the one
a previous image left had gone: a rerun cannot talk to the image it is about to
replace. `FERROHAL_SERIAL_GLOB` overrides the by-id pattern the host matches.

## SPI and EXTI on the Nucleo-F401RE

`hil/run-f401.sh e f g k`. With the links in its `board.rs` fitted. Run E also samples port A with TIM1's update request on DMA2 stream 5 while SPI1 runs at its slowest: SCK (PA5) and S1's MISO end (PA6) show the wire, and in all four modes and both bit orders the 16 bits at the capture edges must be the two bytes in the order set, with no MOSI change off the launch edge [SPI-010]. Pass: every check in every run passes and the script exits zero.

## SPI and EXTI on the Nucleo-H753ZI

`hil/run-h753.sh f g h l`. With the links in its `board.rs` fitted. Run F also samples port A with TIM2's update request on DMA1 stream 1 while SPI1 runs at its slowest, as the F401's run E does [SPI-010]. Run H then turns both caches on and runs SPI2's suite again, its buffers in the MPU's non-cacheable block. Pass: every check in every run passes and the script exits zero.

## Clocks, timers, watchdog and I2C on the Nucleo-F767ZI

`hil/run-f767.sh`, no links (`nucleo-f767zi/src/board.rs`). The glue probe first. Run A
sets voltage scale 3 before `freeze`, which must then read back scale 1 with VOSRDY
[RCC-003] and the over-drive on and ready (ODEN, ODSWEN, ODRDY, ODSWRDY) at 216 MHz
[RCC-004], then ticks TIM1 to TIM5 and TIM8 to TIM14 against the cycle counter as the
F401's timer run does. Run B is the watchdog across its reset, as the F401's; run C the
I2C master on I2C1 with no slave fitted, as the H753's. The F7's Cortex-M7 needs its DWT
unlocked before the cycle counter runs (measured: without it every timed check read 0).
Pass: every check in
every run passes and the script exits zero.

## SPI on the Nucleo-F767ZI

Proposed: no bench code yet, which waits for the owner's go. Link S1 (PB5 to PB4,
`docs/plans/2026-11-17-f411-f722-f745.md`) fitted; the board's clocks frozen at 216 MHz
from the ST-LINK's 8 MHz MCO in bypass, so the over-drive is on [RCC-004]. SPI1's suite
as the F401's run E, with transfers of odd and even lengths [SPI-V3-002] and SCK
sampled between them [SPI-V3-001]. Pass: every check passes and the script exits zero.

## Timers on the Nucleo-F401RE

`hil/run-f401.sh h i n o p r`, with J3 and S1 fitted for O and P; run R needs no links. Measures each timer against the cycle counter, sends waveforms over those wires — run O plays one out over J3 and reads it back, then plays with the lane's output off and checks nothing crosses to the pulled-up far end — and drives the SpeedyBee's four motor lanes from one start, every pin read back by the sampler. Run H first sets scale 3 before `freeze`, for it to replace, and then reads back PWR: the voltage scale `freeze` wrote must be scale 2 (VOS bits 15:14 at 0b10) with VOSRDY set. Run H then frees TIM4, reopening it as the 8 kHz control tick, and frees TIM2 and TIM5 as 1 MHz timebases: the token returns from `free`, so none is stolen. Every tick in run H, TIM10's on the `TIM1_UP_TIM10` vector it shares with TIM1 among them, is then served twice for one update with its vector held off, which must report true and then false, and once stopped has its vector pended by hand, as another timer sharing it would raise it: the handler must run and the tick must not count the pass [TIMER-006]. Run I stops its TIM3 lane group mid-waveform by `abort` and then by `free`; each time no sample of PB0/PB1 may read high, and the freed token must reopen TIM3 at the lane group's period [TIMER-LANES-007 API-002]. Run I then runs one pulse on TIM4 channel 1 (PB6), read by the same sampler [TIMER-PWM-007]. Run R ends with its four lanes as one lane group, TIM3 starting TIM2 through its trigger, and every lane of both must finish [TIMER-LANES-005]. Then TIM1 leaves the sampler and paces one reader of port B on DMA2 stream 5 for that lane group: the reader must register and `prepare_listen` return TIM1's rate [TIMER-LANES-017]; every lane must name lane 0's reader, and after a play with a listen at the end each lane's samples must be that reader's 64 port words [TIMER-LANES-014]; right after the play only TIM2 CH2's stream (DMA1 stream 6) may have TCIE set, and every lane's frame must still end [TIMER-LANES-020]. Pass: every check in every run passes and the script exits zero.

## Timers on the Nucleo-H753ZI

`hil/run-h753.sh i j p q`, with Z2 fitted for Q. Run I first reads back PWR: the voltage scale `freeze` wrote must be the configuration's scale 1 (D3CR VOS 0b11, from the reset's 0b01) with VOSRDY set [RCC-003]. Measures each timer against the cycle counter, and sends waveforms both ways over that wire. Run J also starts TIM1 CH3N for one pulse: the sampler sees one period at the set duty and the counter stopped itself [TIMER-PWM-007]. Run I then frees TIM4, reopening it as the 8 kHz control tick, and frees TIM2 and TIM5 as 1 MHz timebases: the token returns from `free`, so none is stolen. Run J stops its TIM3 lane group as run I does on the F401 [TIMER-LANES-007 API-002]. Run J's TIM3 CH3 lane refuses a waveform stepping from the period's top (ARR) to 0 with `TopToZero`, and the same waveform just clear of `Pwm::top_margin` plays every later duty in its own period, against CH4's steady reference [TIMER-PWM-009]. Run P ends with a lane group over TIM1 and TIM8, linked, a lane on each with its output off, and every lane of both must finish [TIMER-LANES-005]. Run Q then runs four reader rounds over Z2: TIM3's lane plays an 8-duty frame from lent memory, TIM8's lane answers once it has ended, and the reply must be recovered from memory TIM3's lane was lent, each sample within a sixteenth. Round 1 reads on the lane's own stream after the frame's end [TIMER-LANES-016]: the frame and the lent memory must come back and the reply must lie in the lent memory [TIMER-LANES-012]; `prepared` must name the rate `prepare_listen` returned [TIMER-LANES-013]; right after the frame's end TIM3's counter must still be enabled with compare 1 at 0 [TIMER-LANES-018]; CC1E must be set before the listen, CC1E, CC1P and CC1NP clear while listening, and CC1E and CC1P set again after `speak` [TIMER-LANES-021]. Round 2 listens in the run that sees the frame end, and that run must have turned to listening [TIMER-LANES-019]. Round 3 reads through a reader paced by TIM5 at 6 MHz on DMA2 stream 7, at TIM5's rate [TIMER-LANES-017]. Round 4, on the lane group rebuilt, reads through a reader armed ahead on TIM3's update on DMA2 stream 6 [TIMER-LANES-015]. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-F401RE

`hil/run-f401.sh j`. Resets the chip on purpose. Also checks that starting the watchdog froze it for the debugger [IWDG-004]. The first boot's checks cross the reset in the shared watchdog marker, so the exit status counts every boot. The run starts at the board's measured LSI, 30.0 kHz, and requires the starvation to land within 5% of 50 ms [IWDG-006]. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-H753ZI

`hil/run-h753.sh k`. Resets the chip on purpose, three boots in all. Also checks that starting the watchdog froze it for the debugger [IWDG-004]. After the starvation report it starts a 100 ms watchdog whose last 30 ms are its window, feeds it five times 85 ms after each reload, inside the window, and checks that the chip kept running [IWDG-V2-002]; then it feeds 10 ms after a reload, above the window, and the next boot checks that the feed caused the reset [IWDG-V2-001]. The window phase's progress and failures cross the resets in one magic-tagged doubleword, and the first boot's checks in the shared watchdog marker, so a reset anywhere is named and every boot counts in the exit status. probe-rs can lose its session at one of the resets (`SwdApFault`, 1 run in 8 on 2026-11-10); that run exits non-zero with no FAIL line and is rerun. The starvation run starts at the board's measured LSI, 31.7 kHz, and requires the reset within 5% of 50 ms [IWDG-006]. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-F401RE

`hil/run-f401.sh l`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-H753ZI

`hil/run-h753.sh n`. Resets the chip on purpose. Its request crosses the reset with the data cache on, as whole doublewords [RESET-H7-001]. Pass: every check in every run passes and the script exits zero.

## ADC on the Nucleo-F401RE

`hil/run-f401.sh m`. Drives an analog pin from a GPIO through its link. It also reads the die revision through the glue and requires it to equal DBGMCU's IDCODE REV_ID, read directly, and a code RM0368 lists [GLUE-012]. Pass: every check in every run passes and the script exits zero. Last, the scan is freed: the ADC, its stream and its buffer come back and run a sequence again, then ADC1 is freed, reads as powered down (ADON, read directly), and its token reopens it with VDDA about 3.3 V [API-002].

## ADC on the Nucleo-H753ZI

`hil/run-h753.sh o`. Drives an analog pin from a GPIO through its link. It also reads the die revision through the glue and requires it to equal DBGMCU's IDC REV_ID, read directly, and a code RM0433 lists [GLUE-012]. Pass: every check in every run passes and the script exits zero. Last, ADC3's scan is freed: the ADC, its stream and its buffer come back and run a sequence again, then ADC3 is freed, reads as powered down (ADEN, read directly), and its token reopens it with VDDA about 3.3 V [API-002].

## The Foxeer F405 V2 board bench

A flashed bench, tier `FLSH` ([maturity.md](../docs/design/maturity.md)). No requirement names it today: a debugger bench proves every IP it reaches.
`hil/run-foxeer.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. It sets scale 2 before `freeze`, for it to replace, and its clock checks read back PWR: the voltage scale `freeze` wrote must be scale 1 (PWR_CR bits 15:14 at 0b01, bit 14 being this chip's VOS) with VOSRDY set. Nine refusals lead the report: a no-PLL clock config (its token freezes these clocks), a 1 us watchdog, an inexact timebase rate, a 0 Hz SPI config and a 0 Hz polled config, an empty ADC sequence, a 0-rate PWM, a motor lane's CCM buffer, and the lane group's TIM1/TIM8 mismatch re-rated — each constructor handing its parts back, and every part the checks after them use. Pass: every check in every run passes and the script exits zero.

## The TBS Lucid H7 board bench

A flashed bench, tier `FLSH` ([maturity.md](../docs/design/maturity.md)). It carries the SDMMC's evidence, which no debugger bench reaches yet [SDMMC-004]; the rest of what it reaches a debugger bench proves.
`hil/run-lucid.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. Its SD checks need a card in the slot. Nine refusals lead the report, as the Foxeer's through the H7's constructors, with a DTCM lane buffer in place of the CCM one; the ninth is the SDMMC: a bus speed past the card's default limit refuses before the peripheral is touched, and the wiring that comes back opens the real card. Pass: every check in every run passes and the script exits zero. The SDMMC is freed to its wiring and reopened before its card suite, which then runs on the reopened controller.

## The TBS Lucid H7 with its slot empty

A flashed bench, tier `FLSH`, as above [SDMMC-003].
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
Pass: every check in both runs passes and the script exits zero.

## Die 0x413 on the STM32F4 Discovery

`hil/run-discovery.sh a`, with links D1 to D3 fitted (`hil/stm32f4-discovery/src/board.rs`).
The die: `DEV_ID` 0x413, the clocks `freeze` solves at 168 MHz from the ST-LINK's MCO in
bypass, voltage scale 1 (this die's one VOS bit, 14) with VOSRDY, five flash wait states,
and `check_dma_buffer` refusing CCM RAM and accepting SRAM. Then the UART routes the F401
does not have, each opened, echoed, streamed 4 KiB and freed into the next: USART3 and
UART4 over D1 (PC10 to PC11), USART3 over D3 (PD8 to PD9, the LQFP100's), UART5 over D2
(PC12 to PD2), between them every DMA1 route the three have. Pass: every check passes and
the script exits zero.

## USB on the STM32F4 Discovery

`hil/run-discovery.sh b`, with CN5 cabled to the host. The F4's OTG_FS core enumerates
as a CDC device, is freed to its token and pins and reopened, and echoes every block
`hil/usb_cdc_host.py` sends, then counts CPU cycles over 1000 of the host's
start-of-frames and requires 168 MHz within 0.1 % [RCC-001]. Pass: every check passes
and the script exits zero.

## The ROM bootloader on the STM32F4 Discovery

`hil/run-discovery.sh c`, with CN5 cabled to the host. The run asks for the ROM
bootloader and resets; the next boot jumps to it before touching any clock. Pass:
`dfu-util` lists the ROM's DFU device on CN5 under the serial the ROM derives from this
chip's UID within 30 s, and the script exits zero.

## The glue probe on the Nucleo-F401RE

`hil/run-f401.sh probe`: `tools/glue-gen`'s probe of stm32f401re (die 0x433) through this
board's debug probe, the core reset and halted, no firmware. `DEV_ID` is the die's; for
every peripheral the generated glue offers and every bonded GPIO port, a scratch register
ignores a write while its RCC enable bit is clear, keeps it once set, and returns to its
earlier value when its reset bit is pulsed; every vector those peripherals and the DMA
streams raise pends and clears in the NVIC. Pass: every check passes and the script exits
zero.

## The glue probe on the Nucleo-H753ZI

`hil/run-h753.sh probe`: `tools/glue-gen`'s probe of stm32h753zi (die 0x450) through this
board's debug probe, the core reset and halted, no firmware. `DEV_ID` is the die's; for
every peripheral the generated glue offers and every bonded GPIO port, a scratch register
ignores a write while its RCC enable bit is clear, keeps it once set, and returns to its
earlier value when its reset bit is pulsed; every vector those peripherals and the DMA
streams raise pends and clears in the NVIC. Pass: every check passes and the script exits
zero.

## The glue probe on the STM32F4 Discovery

`hil/run-discovery.sh probe`: `tools/glue-gen`'s probe of stm32f407vg (die 0x413) through this
board's debug probe, the core reset and halted, no firmware. `DEV_ID` is the die's; for
every peripheral the generated glue offers and every bonded GPIO port, a scratch register
ignores a write while its RCC enable bit is clear, keeps it once set, and returns to its
earlier value when its reset bit is pulsed; every vector those peripherals and the DMA
streams raise pends and clears in the NVIC. Pass: every check passes and the script exits
zero.

## Complementary outputs on the STM32F4 Discovery

`hil/run-discovery.sh d`. No links. TIM1 drives its complementary output CH1N on PE8 at a
quarter duty, 100 kHz, and TIM8's update request has DMA2 stream 1 copy port E's input
register at 2 MHz: the F401 cannot sample CHxN, its only break timer being its only
timer whose update reaches DMA2, the one DMA that reaches the GPIO ports. Checks: the
duty on the pin, to 1/16, and one one-pulse period. Pass: both checks pass and the
script exits zero.

## Deferred USB on the STM32F4 Discovery

`hil/run-discovery.sh e`, with CN5 cabled to the host. The F4's OTG_FS core is built with
its vector deferred and no start-of-frame heartbeat. The vector's service, the device and
its CDC class share one RTIC resource, so the vector takes the lock the echo task holds
for each poll and echo pass: USB-002's rule of use. The vector wakes the task through a
signal; the task enumerates and echoes every block `hil/usb_cdc_host.py` sends, and the
log gives the longest the task held the lock. Pass: every check passes and the script
exits zero.

## Interrupt-driven I2C on the Nucleo-F401RE

`hil/run-f401.sh x`. Run X drives `InterruptI2c` on I2C3 over
PA8 and PC9 against the Pico 1 simulator, wired as run S, twice: opened
from the plain wiring, every byte moved by the event interrupt, then from
a DMA wiring of DMA1 stream 4 (TX) and stream 2 (RX), channel 3 (RM0368
Table 28). Before each open, an open at a speed the IP cannot time is
refused with the wiring, which then opens. Each mode runs a write, a write
then a read, and a read, of 1, 2, 3, 6, 16 and 32 bytes, ten times each,
every one checked against the simulator (its ECHO register, its pattern
stream) and every phase's path (DMA or interrupt) against the driver's
thresholds through its health counts; then each refusal of `start`, with
the transaction handed back and the bus untouched; a vacant address's
NACK and the next transaction; a stretch past the caller's bound aborted
with its buffers, and the bus running again; and the driver's health
against the run's own counts. A DMA buffer the stream cannot reach is not
exercised: every F401 RAM is reachable. Pass: every check passes and the
script exits zero.

## The lane switch on the Nucleo-F401RE

`hil/run-f401.sh y`, built with `profile-listen` (the script passes it). No links. Run
R's four lanes as one lane group, TIM3 starting TIM2, read through one reader of port B
paced by TIM1 on DMA2 stream 5. Two listens are prepared, of 32 then 64 samples:
`prepared` must name the second, at the rate it returned, and each of 200 exchanges,
listening in the run that sees the frames end, must read 64 samples on every lane
[TIMER-LANES-013]. Over those exchanges the listen's cycle count (`timer::profile`, from entry
to the reader started) must stay within an eighth of its least, since the prepared
switch only stores what was computed ahead [TIMER-LANES-013]. Pass: every check passes and the
script exits zero.

## Timers on the STM32F4 Discovery

`hil/run-discovery.sh f`. No links. Die 0x413's timer trigger routes, each as its glue
states it (RM0090 table 98): TIM8 and TIM3 started by TIM1 (ITR0), TIM2 by TIM3 (ITR2)
and TIM5 by TIM3 (ITR1), every timer at 600 kHz. For each, the slave must not count
while its master is stopped, then start with it in 100 of 100 starts, within 32 counts
[GLUE-010]. Pass: every check passes and the script exits zero.

## The ROM bootloader on the Nucleo-H753ZI

`hil/run-h753.sh z`, with CN13 cabled to the host as the rig keeps it. The run asks for
the ROM bootloader and resets; the next boot jumps to it before touching any clock. The
H7 ROM's DFU serial is not the F4's UID-derived one: it starts with the die's
`DBGMCU_IDC` (measured 2026-11-15: `200364500070` on this H753). Pass: within 30 s a DFU
device not listed before the run appears whose serial starts with the IDCODE read over
the debugger, the script erases the chip so the ROM leaves the bus, and it exits zero.
This H753's bootloader is version 0x91, which no longer locks PWR_CR3 (AN2606), so the
next run needs no power cycle.

