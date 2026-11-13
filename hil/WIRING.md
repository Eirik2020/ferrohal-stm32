# Bench wiring: the Pico 1 I2C simulator

The two Nucleo benches prove FerroHAL's I2C masters against a vacant address
(a NACK the bus itself gives). The Pico 1 closes the other half: a real DW
`apb_i2c` slave on the wire, at address `0x2A`, running the firmware in
`hil/pico-i2c-sim/`. Its register map is in that crate's `src/sim.rs`; both
ends use only the internal ~40 k pull-ups at 50 kHz, so every link below is
a plain DuPont jumper and no resistor is fitted.

The simulator's own documentation - register map, build and flashing - is
[`pico-i2c-sim/README.md`](pico-i2c-sim/README.md).

## Power: the Pico and the Nucleo-H753ZI's CN13 cable

Since 2026-11-13 both stay connected, each on its own USB cable to the host:

- Pico 1 `VBUS` (physical pin 40) is fed by its own micro-USB socket, which
  also flashes it; no 3V3 jumper from either Nucleo is needed. The grounds
  below still tie the I2C return paths short.
- The H753's CN13 (USB OTG) carries its OTG_FS device, which run M (the USB
  device bench) speaks through: it is always connected unless the owner says
  otherwise, so run M is back on `hil/run-h753.sh`'s default list. (Before
  2026-11-13 one cable served both, moved between them.)

## Pico 1 (I2C0) to Nucleo-F401RE — I2C3

Four jumpers. F401 sockets per UM1724 (CN9/CN10 Arduino rows, CN5 power):

| Pico 1 pin | Nucleo-F401RE | Signal |
| --- | --- | --- |
| GP0 (physical 1) | CN10-1 (PC9) | SDA |
| GP1 (physical 2) | CN9-8 (D7, PA8) | SCL |
| GND (physical 3) | CN5-7 (GND) | ground |

Run S on the F401 (`hil/run-f401.sh s`) drives this bus. The Pico answers
with bus tag `1` in its ID register.

## Pico 1 (I2C1) to Nucleo-H753ZI — I2C4

Three jumpers. H753 sockets per UM2407 (Zio CN10; this board has no morpho
headers fitted):

| Pico 1 pin | Nucleo-H753ZI | Signal |
| --- | --- | --- |
| GP2 (physical 4) | CN10-19 (PD13) | SDA |
| GP3 (physical 5) | CN10-21 (PD12) | SCL |
| GND (physical 8) | CN10-22 (GND) | ground |

Run S on the H753 (`hil/run-h753.sh s`) drives this bus. The Pico answers
with bus tag `2` in its ID register. This is the only I2C of the H753 that
reaches a header pair a bench can wire (see below), and the one whose
kernel clock is the APB4/pclk4 path no other bench covers.

## Why no other instance is wired

- **H753 I2C2 (PF1/PF0)**: PF0 is `OSC_IN`, driven at all times by the
  ST-LINK's 8 MHz MCO through solder bridge SB45, which UM2407 fits ON by
  default — and which run A's HSE-bypass clock check depends on, so it
  stays ON. SDA would fight that clock. The only other pair, PB10/PB11, is
  the Z7 UART link (UM2407 clock-configuration chapter, lines on SB44/SB45/
  SB46; `board.rs` run A uses `HSE_CLOCK_CONFIG`).
- **H753 I2C1 (PB6/PB7, PB9/PB8)**: those pairs are the Z3 and Z4 UART
  links.
- **H753 I2C3 (PA8/PC9)**: PA8 exists only on the unfitted morpho headers.
- **F401 I2C2 (PB10/PB11)**: run R's SpeedyBee SPI lanes.
- **F401 I2C1 (PB8/PB9)**: kept slave-free on purpose — run Q's NACK suite
  must stay byte-for-byte what it is.

## Flashing and checking the Pico

The Pico 1 W has no on-board debug probe, and this bench wires none:
flashing goes over its own USB cable through
the bootrom's UF2 drive. The **first** flash needs BOOTSEL by hand —
after that the simulator reboots itself into its bootloader on request,
so the button is never touched again.

    cd hil/pico-i2c-sim
    cargo build --release
    cargo install elf2uf2-rs --locked    # once
    ./../run-i2c-sim.sh flash

`run-i2c-sim.sh flash` builds the firmware, writes the UF2 next to it,
and copies it onto the Pico's `RPI-RP2` drive. When the drive is not
mounted it first runs **run T** on the F401 bench, which writes the
map's `0x3D` BOOT register: the running simulator calls the bootrom's
own `_reset_to_usb_boot` and the drive appears within a second. (If the
Pico holds no simulator at all, or firmware from before run T, hold
BOOTSEL while plugging the cable instead.) One flash serves any number
of bench runs: the runs reset the simulator's counters through the
map's CLEAR register, not by power-cycling it.

`hil/run-i2c-sim.sh` then runs F401 run S and H753 run S and writes the
combined log to `hil/logs/`. The runs' ID checks are the wiring test:
a bus answering with `[0xA1, tag]` is the Pico, wired right.

The firmware looks after itself: a warden task spends a NACK arm on a
three-millisecond timer (the DW never delivers the refused byte, so
nothing on the bus can report the refusal), and an eight-second
watchdog reboots the chip — fed only while both serve tasks are fresh —
so a bus task that ever wedges comes back by itself and the run-T
flash path keeps working.
