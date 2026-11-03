# Bench Procedures

What each requirement's bench entry runs. A run proves the files that declare the
requirement it covers, so after the first pass, a change to one file owes only that
file's requirements a new run.

Every UART check prints the requirements it verifies in square brackets after its
name, so a log shows which layer each line speaks for. A record's image hash is the
hash of the manifest beside its logs, which lists every binary of the run by SHA-256.

## UART on the Nucleo-F401RE

`hil/run-f401.sh a b c d`, with the links in
`hil/nucleo-f401re-loopback/src/board.rs` fitted and the host echo for USART2
running, which the script starts. Pass: every check in every run passes and the
script exits zero. Covers `usart_v2` on all three USARTs, all 10 TX and RX pin
entries and all 9 USART DMA routes, and the checks in `hil/common/tests.rs`.

## UART on the Nucleo-H753ZI

`hil/run-h753.sh a b c d e`, with the Zio links in
`hil/nucleo-h753zi-loopback/src/board.rs` fitted. Pass: every check in every run
passes and the script exits zero. Covers `usart_v4` with the caches off and on, and
the same checks in `hil/common/tests.rs`.

## Chip glue against metapac

`python3 -I tools/check_glue.py`, which compares each chip's routing rows with
stm32-metapac's metadata. Pass: it exits zero. It reads the rows as text, so their
layout must stay as it is.

## SPI and EXTI on the Nucleo-F401RE

`hil/run-f401.sh e f g k`. With the links in its `board.rs` fitted. Pass: every check in every run passes and the script exits zero.

## SPI and EXTI on the Nucleo-H753ZI

`hil/run-h753.sh f g h l`. With the links in its `board.rs` fitted. Pass: every check in every run passes and the script exits zero.

## Timers on the Nucleo-F401RE

`hil/run-f401.sh h i n`. Measures each timer against the cycle counter. Pass: every check in every run passes and the script exits zero.

## Timers on the Nucleo-H753ZI

`hil/run-h753.sh i j p`. Measures each timer against the cycle counter. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-F401RE

`hil/run-f401.sh j`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## Watchdog on the Nucleo-H753ZI

`hil/run-h753.sh k`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-F401RE

`hil/run-f401.sh l`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## Bootloader request on the Nucleo-H753ZI

`hil/run-h753.sh n`. Resets the chip on purpose. Pass: every check in every run passes and the script exits zero.

## ADC on the Nucleo-F401RE

`hil/run-f401.sh m`. Drives an analog pin from a GPIO through its link. Pass: every check in every run passes and the script exits zero.

## ADC on the Nucleo-H753ZI

`hil/run-h753.sh o`. Drives an analog pin from a GPIO through its link. Pass: every check in every run passes and the script exits zero.

## The Foxeer F405 V2 board bench

`hil/run-foxeer.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. Pass: every check in every run passes and the script exits zero.

## The TBS Lucid H7 board bench

`hil/run-lucid.sh`. Over USB and DFU, with ESC power and battery off, and motor outputs off. Its SD checks need a card in the slot. Pass: every check in every run passes and the script exits zero.
