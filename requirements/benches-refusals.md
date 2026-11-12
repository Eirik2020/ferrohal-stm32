# Bench Procedures: the constructor refusals

The refusal runs of [benches.md](benches.md), split out to keep that file in
its budget; the same rules apply.

## Constructor refusals on the Nucleo-F401RE

`hil/run-f401.sh v`, with J1, J2, S3 and S5 fitted. Every public
constructor refuses on hardware, hands its parts back, and the run uses
them: a no-PLL clock config (the RCC token re-freezes at 56 MHz on PLLQ,
so the USB port must refuse too, and its pins come back as the GPIO that
carries a level over J2), a 1-baud UART (the wiring and buffers open the
port, which echoes a frame over J1), a 0 Hz SPI stream config on S5 and
a 0 Hz polled config on S3 (both wirings transfer bytes), an
out-of-range I2C speed (the wiring opens the bus, which NACKs a vacant
address), an empty ADC scan sequence (the converter, stream and buffer
scan the internal channels), a 0-rate PWM (the timer makes the lanes),
a lane buffer in CCM RAM, which no DMA controller reaches (the channel
and stream play on PB0), a 0-rate sampler (the parts decode that
lane), two AWG timers at different periods (the second PWM re-rated and
the pair plays on PB6), an inexact tick and timebase (both timers run,
the tick paused), and a 1 us watchdog (the peripheral starts, feeds for
300 ms, then starves, and the second boot reports the bite — run J's
shape, so this run's own reset clears its flags and leaves none for
the next bench run). Each handed-back UART, SPI and I2C is then freed: its enable bit (UE, SPE, PE) and every DMA stream it ran must read off [API-002]. Pass: every check in both boots passes and the
script exits zero.

## Constructor refusals on the Nucleo-H753ZI

`hil/run-h753.sh u`, with Z2, S1 and S3 fitted. The H7's own set: a
no-PLL1 clock config (the RCC token re-freezes the bench's clocks), an
unreachable-baud USART6 over Z2, a 0 Hz SPI stream config on S1 and a
0 Hz polled config on S3, an out-of-range I2C3 speed (the bus NACKs a
vacant address), an empty ADC scan sequence (the parts scan PA1 twice),
a 0-rate TIM3 PWM, a lane buffer in the DTCM, which no DMA controller
reaches (the channel and stream play on PB0), a 0-rate TIM8 sampler
(the parts decode that lane), TIM3 and TIM5 at different periods (the
second PWM re-rated and the AWG plays PB0), an inexact TIM15 tick and
TIM2 timebase, and a watchdog refused twice over one token — a window
longer than the timeout, then a 1 us timeout — before it starts, feeds
for 300 ms and starves, and the second boot reports the bite, run K's
shape. USB is left out: this chip's constructor starts its own 48 MHz
source, so nothing this bench can wire makes it refuse; the F401's run
V covers it. Each handed-back UART, SPI and I2C is then freed: its enable bit (UE, SPE, PE) and every DMA stream it ran must read off [API-002]. Pass: every check in both boots passes and the script
exits zero.
