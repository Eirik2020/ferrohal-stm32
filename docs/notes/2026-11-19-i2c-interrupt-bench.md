# Interrupt-driven I2C: what the prototypes measured (2026-11-19)

Source: ADR-013, [`../decisions/2026-11-19-i2c-interrupt-driver.md`](../decisions/2026-11-19-i2c-interrupt-driver.md).

Four variants of an interrupt-driven I2C master were built and run on the F401
(`i2c_v1`), the F767 (`i2c_v2` with DMA) and the H753 (`i2c_v2`, interrupt-fed: its one
benchable I2C, I2C4, has only BDMA routes, RM0433 Table 128), against the Pico 1
simulator at 50 kHz: data by DMA or by the event interrupt, on lent buffers or on
driver-owned ones with a copy each way. The prototype, its patches and the full tables
are kept outside the tree.

CPU cycles a transaction costs the caller (means of ten):

| | F401, 84 MHz | F767, 216 MHz |
|---|---|---|
| DMA, a write or a read, any length 2-32 | about 2 900 | about 3 900 |
| DMA, a write then a read | about 6 100 | about 6 400 |
| interrupt-fed, a byte | about 470 | about 580 |
| blocking, 32 bytes read | 502 800 | 1 316 600 |
| owned over lent: `start`, and the copy out | +140 to +230, 100-200 | +370 to +660, 350-525 |

A phase of one byte is cheaper interrupt-fed on both; from two bytes DMA wins, except a
two-byte read on `i2c_v1`, which waits once at BTF and costs 2 406 against 2 931. On
`i2c_v2`, an abort that only cycled PE left the slave mid-read: the next transaction
NACKed and, in earlier rounds, the bus stayed held until the Pico was replugged. Setting
CR2.STOP instead brought the bus back at once on both `i2c_v2` boards.

The F767 showed intermittent 32-byte miscompares: three in three runs, a different mode
and byte each time, never on the H753 or F401.
