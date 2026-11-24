# ADR-014: Timer lanes as the lane port needs them (2026-11-19)

ADR-014 - Status: promoted

Promoted to: [lanes.md](../design/lanes.md).

## Context

A bidirectional one-wire protocol on motor outputs sends a frame on each lane and reads the
reply on the same pins within tens of microseconds. ferroforge-stm32's lane port needs
FerroHAL's lanes to switch from sending to reading inside that window, on the pins,
timers and DMA channels real boards give their motors. Two rounds of prototypes on the
STM32F4 Discovery and the Nucleo-H753ZI measured what that takes.

Before this, a listen computed its rates and transfers at the switch, every lane read on
its own DMA channel, a lane copied its samples into its own buffer, and the timers stopped
at a frame's end and restarted at the listen.

## Decision

1. One behaviour. The prototypes' build options become the only behaviour. Only the
   profiling features stay, listed in `docs/policies/features.md`.
2. Lent memory only. `Frame` is checked and closed where it is written, and a lane
   plays it in place; a receive fills lent memory. A lane keeps no memory of its own.
3. The switch is prepared. `prepare_listen(rate, count)` computes everything;
   `listen()` takes no arguments and only stores.
4. One reader per timer and port, served in one of three forms: armed ahead on a spare
   lane-timer request, the port's first lane's own DMA channel, or paced by a timer that is
   not a lane timer.
5. Timers left running at the frames' end, retimed at the switch; any load or play
   leaves that state first.
6. Listen at the end, in the last lane's interrupt: one CCER store per timer, CCxP and
   CCxNP clear while off.
7. TIM5's trigger from TIM3 on die 0x413 (RM0090 table 98, ITR1), so a lane group on
   TIM3 and TIM2 can start the responder or a third timer on the same edge.
8. A start rebuilds no transfer. The lane's send transfer is prepared at assembly, so a
   start sets only memory and length.
9. A reader's DMA channel comes back (API-002): every refused registration hands the
   DMA channel back, and `LaneGroup::free` returns each reader's DMA channel once, typed.

## Rejected

- Input capture as the receive: a second receive path that adds no board, cannot serve
  a complementary pin, and left 0.8 us of margin where sampling needs a pacer.
- Release by a COM event, and releasing the pin by MODER first: measured, neither
  gained a sweep step nor removed the H753's 210 ns pulse.
- Keeping the options as features: every user would get the slow switch by default,
  and each option doubles the configurations the benches must prove.
- Repeat and stream rings in this change: several lanes in lockstep are not built.

## Consequences

- The lane API breaks; the changes are listed in the design document.
- The lane requirements TIMER-LANES-012 to TIMER-LANES-021 state the behaviour above; the
  lane port names them as its library requirements.
