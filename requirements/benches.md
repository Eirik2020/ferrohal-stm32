# Bench Procedures

What each requirement's bench entry runs. A run proves the files that declare the
requirement it covers, so after the first pass, a change to one file owes only that
file's requirements a new run.

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
