# Bench Procedures: the injected faults

The fault-injection runs of [benches.md](benches.md), split out to keep that
file in its budget; the same rules apply.

## Fault injection on the Nucleo-F401RE

`hil/run-f401.sh w`, with J3 and J4 fitted. Run W injects into the
cross-wired pair, one fault at a time, reaching the registers through
`stm32_metapac` as run B's parity watch does. Overrun: USART6's DMA receive
request is disconnected (`CR3.DMAR` off) for two frames, so the second
completes while the first still holds DR and the USART's own overrun flag
fires; the run waits for the driver to count the fault before the request
returns, because the restore otherwise races the flag, then the frames
after it are checked whole, at most the first byte lost to the clear's DR
read. Noise: USART1 sends 8N2 at 120500, 4.6 % fast for the receiver's
115200 — the window where the last of the three samples of the last data
bit straddles the stop, so the noise flag fires with every byte intact and
no framing error; below 4.35 % no sample straddles, above 5.1 % the middle
one crosses too and the byte corrupts. DMA transfer error: USART6's
receive stream is stopped, re-pointed at 0x20200000, past the 96 KiB of
SRAM, and restarted; the next byte is a bus error, the driver reports
DmaTransfer and restarts the ring on its own buffer, and an echo after the
gap succeeds — what the outage costs is timing (a frame completed into a
dead stream raises overrun, whose mark the restart wipes), so the bytes
right after the gap go to the log unnamed. Timeout: USART1's DMA transmit
request is disconnected before a 128-byte frame is queued, so nothing
drains; the queue's bound expires, the driver stops the stream, drops the
queue, counts the timeout and reports it from `flush`, and the next frame
goes out whole. Pass: every check passes and the script exits zero.

## Fault injection on the Nucleo-H753ZI

`hil/run-h753.sh w`, with Z2 fitted. Run W injects into the USART6 loopback
what a UART can do to itself; the noise work needs two cross-wired UARTs,
which this board's links do not offer, so it runs on the F401 only.
Overrun: USART6's DMA receive request is disconnected for two frames and
the run waits for the flag to be counted before the request returns; v4
clears through ICR, which touches no byte, so A survives, and whether B is
discarded or lands once the request returns is the shift register's
timing, which the check accepts both ways. DMA transfer error: USART6's
receive stream is re-pointed at the DTCM, which DMA1 and DMA2 cannot reach
— the same fact `ConfigError::BufferUnreachable` refuses at construction —
and the next byte is a bus error; the driver reports DmaTransfer, restarts
the ring on its own buffer, and an echo after the gap succeeds. Timeout:
USART6's DMA transmit request is disconnected before a 128-byte frame is
queued; the bound expires, the queue is dropped and counted, and the next
frame goes out whole. Pass: every check passes and the script exits zero.
