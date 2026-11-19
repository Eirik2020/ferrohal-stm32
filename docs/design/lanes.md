# Timer lanes: frames, the switch and the readers

Source: ADR-013, [`../decisions/2026-11-19-timer-lanes-round2.md`](../decisions/2026-11-19-timer-lanes-round2.md).

A compare lane plays one duty per timer period from memory over DMA, and a two-way lane
then releases its pin and reads the line it sent on (`timer::CompareLane`). A waveform
generator (`timer::Awg`) runs the lanes of one or two timers from one start. This is how
a bidirectional protocol (DShot with telemetry, one wire) runs on FerroHAL; the protocol
itself is the user's, never FerroHAL's. The behaviour below is the only one: no build
option changes it, beyond the profiling features (`docs/policies/features.md`).

## Frames in lent memory

- A `Frame` is checked and closed where it is written (`Frame::new`): at least one duty,
  room for the closing 0, no duty near the top before a 0 or last (`Error::TopToZero`).
  The closing 0 is written then. `Frame::new` is pure, so a frame builds anywhere.
- A lane plays a frame in place (`arm_frame`, `Awg::load_frame`); nothing is copied, and
  `take_frame` hands the memory back once the lane has stopped. A start checks only that
  the lane is free, the frame's limits are the timer's, and a DMA stream reaches its
  memory, one compare each; the lane's send transfer is prepared once, at assembly, and a
  start sets only its memory and length.
- A receive fills memory the caller lends (`lend_samples`, `take_samples`). A lane keeps
  no buffer of its own for a frame or a receive.

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
interrupt off; the switch opens the request (`timer::pwm::Pacer`).

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

## Measured (round 2 prototypes, 2026-11-18)

Margin from a frame's end to the latest reply read whole, against 30 us, four lanes on the
STM32F4 Discovery (F407) and two on the Nucleo-H753ZI; T is the caller's switch, F the
interrupt run's (`listen_on_end`). A sweep step is 1.67 us.

| form | F407 T / F | H753 T / F |
|---|---|---|
| armed ahead | 9.2 / 10.8 us | 12.5 / 14.2 us |
| the lane's own stream | 4.2 / 5.8 us | 7.5 / 9.2 us |
| paced (TIM8, lanes on TIM3 and TIM2) | 9.2 / 10.8 us | not built |

## Left out

- Input capture as a receive: cheaper only with one lane on a timer, cannot serve a
  complementary pin, and left 0.8 us of margin on the one layout where sampling needs a
  pacer.
- Repeat, stream and endless receive (DMA double buffer mode): their own calls, not in
  FerroHAL yet.

## Open

- In `docs/DEBT.md`: the H753's 210 ns pulse at the release.
- On the roadmap (`docs/CHIPS.md`, Not supported yet): a reader paced by a compare
  channel of a timer that is not a lane timer; the TIM3 to TIM5 trigger route on the H743
  and H753 (RM0433 table 341, ITR2).
