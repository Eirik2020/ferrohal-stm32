#!/usr/bin/env bash
# Runs the bench of a board with no probe: builds its firmware, flashes it
# through the ROM DFU bootloader over the board's USB port, then reports over
# that port. Exits non-zero if flashing or any check failed.
#
#     run-dfu.sh <crate and USB serial> <its flash layout in the DFU listing>
#
# The layout keeps one board's image off another: only a DFU device showing it
# gets flashed. Reaches the bootloader by itself from the bench's firmware. From
# anything else, it asks: hold BOOT while plugging in the USB cable. Log goes to
# hil/logs/<board>.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
board="$1"
layout="$2"
crate="$here/$board"
elf="$crate/target/thumbv7em-none-eabihf/release/$board"
mkdir -p "$here/logs"

(cd "$crate" && cargo build --release) || exit 1
arm-none-eabi-objcopy -O binary "$elf" "$elf.bin" || exit 1

dfu_serial() {
    dfu-util -d 0483:df11 -l 2>/dev/null | grep -F "$layout" | sed -n 's/.*serial="\([^"]*\)".*/\1/p' | head -1
}

for port in /dev/serial/by-id/*FerroHAL*"$board"*; do
    [ -e "$port" ] && printf BENCHDFU > "$port"
done
serial=""
for _ in $(seq 1 100); do
    serial="$(dfu_serial)"
    [ -n "$serial" ] && break
    sleep 0.3
done
if [ -z "$serial" ]; then
    echo "FAIL no $board in DFU: hold BOOT while plugging in the USB cable, then run this again"
    exit 1
fi

# `dfu-util` exits non-zero when the board leaves DFU before the last status.
dfu-util -d 0483:df11 -S "$serial" -a 0 -s 0x08000000:leave -D "$elf.bin" 2>&1 | tail -2

python3 -I "$here/usb_cdc_host.py" 30 "$board" report 2>&1 | tee "$here/logs/$board.log"
exit "${PIPESTATUS[0]}"
