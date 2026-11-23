# Timer lanes: frames, the switch and the readers

Source: ADR-014, [`../decisions/2026-11-19-timer-lanes-round2.md`](../decisions/2026-11-19-timer-lanes-round2.md).

A compare lane plays one duty per timer period from memory over DMA, and a two-way lane
then releases its pin and reads the line it sent on (`timer::CompareLane`). A waveform
generator (`timer::Awg`) runs the lanes of one or two timers from one start. The lanes are
for one-wire protocols that send a frame on a pin and read the reply on the same pin
within tens of microseconds; any such protocol is the user's, never FerroHAL's, and none
has run on these lanes against a real device yet. The behaviour below is the only one: no
build option changes it, beyond the profiling features (`docs/policies/features.md`).

## Frames in lent memory

- A `Frame` is checked and closed where it is written (`Frame::new`): at least one duty,
  room for the closing 0, no duty near the top before a 0 or last (`Error::TopToZero`).
  The closing 0 is written then. `Frame::new` is pure, so a frame builds anywhere.
- A lane plays a frame in place (`CompareLane::arm`, `Awg::load`), and only a lent
  frame: nothing is copied, and `take_frame` hands the memory back once the lane has
  stopped. A start checks only that
  the lane is free, the frame's limits are the timer's, and a DMA stream reaches its
  memory, one compare each; the lane's send transfer is prepared once, at assembly, and a
  start sets only its memory and length.
- A receive fills memory the caller lends (`lend_samples`, `take_samples`); a listen with
  none lent is refused (`Error::NotLent`). A lane keeps no memory of its own: assembly
  takes the pin and the stream only, and `free` gives back the wiring with whatever is
  still lent.

## The prepared switch

- `prepare_listen(rate, count)` computes, ahead of the switch, each timer's capture rate,
  every reader's transfer and the request masks. `listen` only stores them: it holds no
  arithmetic, so its cycle count is the same every time.
- `speak` turns back: the send rate again, every output on at its inactive level.

## One reader per timer and port

Every lane on one timer and one GPIO port shares one reader. What it reads is that port's
words, and a lane's samples are one bit of them (`samples`, `reader_of`). A paced reader
(below) serves its port for both timers.

## Three reader forms

Chosen per port by what the board has free; the lane API is the same for all three.

| form | call | needs | when |
|---|---|---|---|
| armed ahead | `read_ahead_first_update`, `_second_update`, `_first_compare`, `_second_compare` | a spare request of a lane timer (its update, or a compare channel no lane uses), one stream that reaches GPIO, its vector | a stream with such a request is free |
| the lane's own stream | none: the port's first lane reads on its own stream, started at the switch | the lane's stream reaches GPIO | nothing else is free |
| paced | `read_ahead_paced` | a timer that is not a lane timer, running at the capture rate; one stream that reaches GPIO on its update request; its vector | the lanes' own streams cannot reach GPIO |

A reader armed ahead is enabled while the frame plays, its request closed, so the switch
opens one DIER bit. A paced reader's timer runs from setup with its update request and
interrupt off; the switch opens the request (`timer::pwm::Pacer`). The pacer stays the
caller's.

A reader takes its stream's token, and a refused registration gives it back. `Awg::free`
returns every reader's stream, stopped, in a `ReaderStreams`, from which each comes out
once as the token it went in as (`take::<D, N>()`).

Which form a board gets:

- F4: only DMA2 reaches GPIO (RM0090 figure 33 note 1), and on the F405/F407 only TIM1
  and TIM8 have requests on DMA2 (RM0090 table 44). Lanes on TIM1 or TIM8 read on their
  own streams, or are armed ahead where a DMA2 stream with a spare TIM1 or TIM8 request
  is free. Lanes on any other timer send on DMA1 and need a reader paced by TIM1 or TIM8
  on a free DMA2 stream, one per GPIO port the lanes use. A board with neither cannot
  receive.
- H7: every DMA1 and DMA2 stream reaches GPIO and DMAMUX routes any request, so a reader
  can be armed ahead whenever a stream is free.

## Timers left running

At the frames' end, with every reader the plan uses armed, the timers keep running at the
send rate, their requests closed, every output at duty 0 (the closing sample). `listen`
then retimes them by PSC, ARR and an update generation, with no stop or restart. A load
or a play leaves that state first, so a loaded first duty is never latched early.

## Listen at the end, and the last lane's interrupt

- With `listen_on_end`, the interrupt run that sees the last lane end starts the prepared
  listen itself, instead of the caller after it returns.
- Only the lane that ends last (the highest numbered; the second timer starts from the
  first's trigger, after it) raises its end-of-transfer interrupt. A lane found still
  running when it ends gets its interrupt back, so frames of unequal length still end.

## Outputs and polarity

- One CCER store per timer releases every output, and one restores them.
- A complementary output is restored after a receive.
- CCxP and CCxNP are clear while a channel is off (RM0090 table 95 note 1, RM0433 table
  337 note 1).

## Measured (product, 2026-11-19)

Margin from a frame's end to the latest reply read whole, against 30 us, with no
profiling feature: ferroforge-stm32's lane port benches at this crate's c723b1c, four
lanes on the STM32F4 Discovery (F407) and two on the Nucleo-H753ZI, 600 kHz, a sweep step
1.67 us. F is the interrupt run's switch (`listen_on_end`, through the lane port), T the
caller's (these lanes under RTIC, no port). The switch is `Awg::listen` whole, measured with
`profile-listen` alone: DWT cycles, mean of 37,000. Each margin is at the round 2
prototypes' sweep step or past it (theirs in brackets).

| form | F407 F / T | F407 switch F / T | H753 F / T | H753 switch F / T |
|---|---|---|---|---|
| armed ahead | 10.84 / 10.84 us (10.8 / 9.2) | 686 / 639 | 14.18 / 12.52 us (14.2 / 12.5) | 1513 / 1440 |
| the lane's own stream | 5.84 / 4.18 us (5.8 / 4.2) | 1661 / 1611 | 10.85 / 9.19 us (9.2 / 7.5) | 2958 / 2860 |
| paced (TIM8, lanes on TIM3 and TIM2) | 10.84 / 9.18 us (10.8 / 9.2) | 711 / 715 | not built | not built |

## Left out

- Input capture as a receive: cheaper only with one lane on a timer, cannot serve a
  complementary pin, and left 0.8 us of margin on the one layout where sampling needs a
  pacer.
- Repeat, stream and endless receive (DMA double buffer mode): their own calls, not in
  FerroHAL yet.

## Open

- In `docs/DEBT.md`: the H753's 210 ns pulse at the release.
- On the roadmap (`docs/ROADMAP.md`): a reader paced by a compare
  channel of a timer that is not a lane timer; the TIM3 to TIM5 trigger route on the H743
  and H753 (RM0433 table 341, ITR2).

## What changed from the earlier lane API (ADR-014)

The prototypes' build options (`lane-reader-ahead`, `lane-retime-running`,
`lane-listen-at-end`, `lane-last-interrupts`, `lane-paced-reader`, `lane-shared-reader`)
are the only behaviour; none is a feature.

- `Awg::listen(rate, count)` became `Awg::prepare_listen(rate, count)` then
  `Awg::listen()`; `CompareLane::listen(pwm, count, duty)` became
  `CompareLane::prepare_listen(count)` then `CompareLane::listen(pwm, duty)`.
- Lanes on one timer and port share one reader: `samples(lane)` is that reader's words.
- Lane assembly takes no buffers (`assemble`, `assemble_two_way`, `assemble_paced` take
  the timer only), and its refusal is the wiring alone (`LaneRefused`,
  `ComplementaryLaneRefused`; `TwoWayLaneRefused` is gone). `CompareLane::arm` and
  `Awg::load` take a `Frame` (the copying forms and `arm_frame`, `load_frame` are gone);
  `CompareLane::free` returns the wiring with the lent frame and samples.
- `read_ahead_*` refuse with their stream (`Refused<Error, S>`); `Awg::free` returns a
  fifth element, the `ReaderStreams`.
- A listen with no samples memory lent is refused with `Error::NotLent`.
- At a frame's end the timers keep running (outputs at duty 0) until the next listen,
  load or play.
- A start keeps one compare each for the lane being free, the frame's limits and the
  memory's DMA reach.
- Every route die 0x413 states for the TIM5 trigger runs on the Discovery (GLUE-010).
