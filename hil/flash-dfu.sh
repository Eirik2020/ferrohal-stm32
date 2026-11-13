#!/usr/bin/env bash
# Runs the bench of a board with no probe: builds its firmware, flashes it
# through the ROM DFU bootloader over the board's USB port, then reports over
# that port. Exits non-zero if flashing or any check failed.
#
#     flash-dfu.sh <crate and USB serial> <its flash layout in the DFU listing>
#
# The layout keeps one board's image off another: only a DFU device showing it
# gets flashed. Boards can share a layout (the H753ZI and the Lucid both say
# 16*128Kg), so with more than one match the script lists them and stops; and
# <BOARD>_DFU_SERIAL - the serial dfu-util prints for the board - pins the pick
# when set (LUCID_H7_DFU_SERIAL for run-lucid.sh). That serial is not always
# board-unique: the Lucid's H7 prints its IDCODE (200364500000, measured
# 2026-11-10) and the H753ZI's IDCODE measured the same, so the count guard is
# what protects that pair. Reaches the bootloader by itself from the bench's
# firmware. From anything else, it asks: hold BOOT while plugging in the USB
# cable. Log goes to hil/logs/<board>.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
board="$1"
layout="$2"
crate="$here/$board"
elf="$crate/target/thumbv7em-none-eabihf/release/$board"
mkdir -p "$here/logs"

(cd "$crate" && cargo build --release) || exit 1

dfu_serial() {
    # One device lists one line per alt setting with the serial repeated, so
    # matches are counted per device - dfu-util's devnum - not per line.
    local pin_var pin listing picks count
    pin_var="${board//-/_}"
    pin_var="${pin_var^^}_DFU_SERIAL"
    pin="${!pin_var:-}"
    listing=$(dfu-util -d 0483:df11 -l 2>/dev/null)
    if [ -n "$pin" ]; then
        picks=$(printf '%s\n' "$listing" | grep -F "serial=\"$pin\"" | sed -n 's/.*devnum=\([0-9]*\),.*serial="\([^"]*\)".*/\1 \2/p' | sort -u)
    else
        picks=$(printf '%s\n' "$listing" | grep -F "$layout" | sed -n 's/.*devnum=\([0-9]*\),.*serial="\([^"]*\)".*/\1 \2/p' | sort -u)
    fi
    count=$(printf '%s\n' "$picks" | grep -c . || true)
    if [ "$count" -gt 1 ]; then
        echo "FAIL dfu: $count devices in DFU match $layout (or the pin); set $pin_var or plug one board in:" >&2
        printf '%s\n' "$picks" | cut -d' ' -f2- >&2
        return 1
    fi
    printf '%s\n' "$picks" | head -1 | cut -d' ' -f2-
}

# Flashes one firmware of the crate, from whatever of this bench is running.
flash() {
    local elf="$crate/target/thumbv7em-none-eabihf/release/$1"
    arm-none-eabi-objcopy -O binary "$elf" "$elf.bin" || return 1
    local port
    for port in /dev/serial/by-id/*FerroHAL*"$board"*; do
        [ -e "$port" ] && printf BENCHDFU > "$port"
    done
    local serial=""
    for _ in $(seq 1 100); do
        if serial="$(dfu_serial)"; then
            [ -n "$serial" ] && break
        else
            return 1
        fi
        sleep 0.3
    done
    if [ -z "$serial" ]; then
        echo "FAIL no $board in DFU: hold BOOT while plugging in the USB cable, then run this again"
        return 1
    fi
    # The ROM leaves DFU at `leave` without answering the last GET_STATUS, so
    # dfu-util ends with that error on every good flash; the download itself
    # must have completed.
    local out
    out=$(dfu-util -d 0483:df11 -S "$serial" -a 0 -s 0x08000000:leave -D "$elf.bin" 2>&1)
    printf '%s\n' "$out" | tail -2
    if ! printf '%s\n' "$out" | grep -q 'File downloaded successfully'; then
        echo "FAIL dfu-util did not report a completed download of $1"
        return 1
    fi
    # The old firmware's port goes first, or the host could find it again.
    sleep 1
}

status=0
# USB under RTIC: only its interrupt services it, the host checks every byte.
flash usb-irq || exit 1
python3 -I "$here/usb_cdc_host.py" 30 "$board" 2>&1 | tee "$here/logs/$board-usb-irq.log"
[ "${PIPESTATUS[0]}" -eq 0 ] || status=1

flash "$board" || exit 1
python3 -I "$here/usb_cdc_host.py" 30 "$board" report 2>&1 | tee "$here/logs/$board.log"
[ "${PIPESTATUS[0]}" -eq 0 ] || status=1
exit "$status"
