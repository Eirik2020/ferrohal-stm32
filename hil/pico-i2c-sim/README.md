# pico-i2c-sim

A Raspberry Pi Pico 1 (RP2040) standing in for the I2C peripherals the
FerroHAL benches otherwise have no slave for: two DW `apb_i2c` slaves at
address `0x2A`, one per bench, speaking the register map in `src/lib.rs`.
The wiring it needs is in [`../WIRING.md`](../WIRING.md).

- Host unit tests (the register map):

      cargo test --target x86_64-unknown-linux-gnu

- Build the firmware (a `thumbv6m-none-eabi` toolchain must be
  installed) and put it on the Pico over BOOTSEL/UF2 — the Pico has no
  on-board debug probe, and this bench wires none:

      cargo build --release
      cargo install elf2uf2-rs --locked    # once
      elf2uf2-rs target/thumbv6m-none-eabi/release/pico-i2c-sim firmware.uf2

  Hold BOOTSEL while plugging the Pico's USB cable and drop the UF2
  onto the `RPI-RP2` drive — that is the **first** flash only. After
  it, the map's `0x3D` BOOT register reboots the simulator into its
  bootloader on request, and `../pico-i2c-sim.sh flash` sends that
  request itself (run T on the F401), so the button is never needed
  again.

`../pico-i2c-sim.sh` then runs the two bench runs that speak to this
firmware, F401 run S and H753 run S, writing one log. The firmware
keeps its state across runs: each run resets the counters through the
map's CLEAR register, not by power-cycling the Pico — and a warden task
plus an eight-second watchdog keep it able to answer (the refusal's
timed disarm, and a self-reboot when a bus task wedges).
