# ADR-014: Timer lanes as the lane port needs them (2026-11-19)

ADR-014 - Status: promoted

Promoted to: [lanes.md](../design/lanes.md).

## Context

A bidirectional one-wire protocol on motor outputs sends a frame on each lane and reads the
reply on the same pins within tens of microseconds. ferroforge-stm32's lane port (its
frame port, D28) needs FerroHAL's lanes to switch from sending to reading inside that
window, on the pins, timers and DMA streams real boards give their motors. Two rounds of
prototypes on the STM32F4 Discovery and the Nucleo-H753ZI (2026-11-17 to 11-19, local
branches) measured what that takes. The owner accepted the lane port's design on
2026-11-19 (D30 to D33 of the port's design record) and asked for it as product code.

Before this, a listen computed its rates and transfers at the switch, every lane read on
its own stream, a lane copied its samples into its own buffer, and the timers stopped at a
frame's end and restarted at the listen.

## Decision

1. **One behaviour.** The prototypes' build options (`lane-reader-ahead`,
   `lane-retime-running`, `lane-listen-at-end`, `lane-last-interrupts`,
   `lane-paced-reader`, `lane-shared-reader`) become the only behaviour. Only the
   profiling features stay, listed in `docs/policies/features.md`.
2. **Lent memory only.** `Frame` is checked and closed where it is written, and a lane
   plays it in place; a receive fills lent memory. A lane keeps no memory of its own
   (D30): the copying path is gone, so sending and receiving each have one path.
3. **The switch is prepared.** `prepare_listen(rate, count)` computes everything;
   `listen()` takes no arguments and only stores.
4. **One reader per timer and port**, served in one of three forms: armed ahead on a spare
   lane-timer request, the port's first lane's own stream, or paced by a timer that is not
   a lane timer.
5. **Timers left running** at the frames' end, retimed at the switch; any load or play
   leaves that state first.
6. **Listen at the end, the last lane's interrupt**, one CCER store per timer, CCxP and
   CCxNP clear while off.
7. **TIM5's trigger from TIM3** on die 0x413 (RM0090 table 98, ITR1), so a generator on
   TIM3 and TIM2 can start the responder or a third timer on the same edge. Every route
   that die states runs on the Discovery (GLUE-010).
8. **A start rebuilds no transfer (D33).** The lane's send transfer is prepared at
   assembly, so a start sets only memory and length; it keeps one compare each for the
   lane being free, the frame's limits and the memory's DMA reach.
9. **A reader's stream comes back** (API-002). A reader keeps its stream's id, every
   refused registration hands the stream back, and `Awg::free` returns each reader's
   stream once, typed, through `ReaderStreams`.

## Breaking changes

- `Awg::listen(rate, count)` becomes `Awg::prepare_listen(rate, count)` then
  `Awg::listen()`; `CompareLane::listen(pwm, count, duty)` becomes
  `CompareLane::prepare_listen(count)` then `CompareLane::listen(pwm, duty)`.
- Lanes on one timer and port share one reader: `samples(lane)` is that reader's words.
- Lane assembly takes no buffers (`assemble`, `assemble_two_way`, `assemble_paced` take
  the timer only), and its refusal is the wiring alone (`LaneRefused`,
  `ComplementaryLaneRefused`; `TwoWayLaneRefused` is gone). `CompareLane::arm` and
  `Awg::load` take a `Frame` (the copying forms and `arm_frame`, `load_frame` are gone);
  `CompareLane::free` returns the wiring with the lent frame and samples.
- `read_ahead_*` refuse with their stream (`Refused<Error, S>`); `Awg::free` returns a
  fifth element, the `ReaderStreams`.
- A listen with no samples memory lent is refused with the new `Error::NotLent`.
- At a frame's end the timers keep running (outputs at duty 0) until the next listen,
  load or play.

## Rejected

- **Input capture as the receive** (the prototypes' stage F): a second receive path that
  adds no board, cannot serve a complementary pin, and left 0.8 us of margin where
  sampling needs a pacer.
- **Release by a COM event**, and **releasing the pin by MODER first**: measured, neither
  gained a sweep step nor removed the H753's 210 ns pulse.
- **Keeping the options as features**: every user would get the slow switch by default,
  and each option doubles the configurations the benches must prove.
- **Repeat and stream rings** in this change: their cause of an intermittent DMA fault
  was found only on 2026-11-19, and several lanes in lockstep are not built.

## Consequences

- The lane requirements AWG-012 to AWG-021 state the behaviour above; the lane port names
  them as its library requirements.
- `timer/awg.rs`, `timer/pwm.rs`, `timer/mod.rs` and `dma/mod.rs` change, so the timer
  and DMA benches on the F401, the H753 and the Discovery owe their runs.
