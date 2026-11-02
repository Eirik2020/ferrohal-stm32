#!/usr/bin/env bash
# Runs the Foxeer F405 V2 bench: builds the firmware, flashes it through the
# ROM DFU bootloader over the board's USB port, then reports over that port.
# Exits non-zero if flashing or any check failed.
#
# Reaches the bootloader by itself from this bench's firmware. From anything
# else, it asks: hold BOOT while plugging in the USB cable, or use the
# running firmware's own way in (FerroWasp: `ferro-configurator device reboot
# --dfu`). Log goes to hil/logs/foxeer.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
crate="$here/foxeer-f405-v2"
elf="$crate/target/thumbv7em-none-eabihf/release/foxeer-f405-v2"
serial=foxeer-f405-v2
mkdir -p "$here/logs"

(cd "$crate" && cargo build --release) || exit 1
arm-none-eabi-objcopy -O binary "$elf" "$elf.bin" || exit 1

in_dfu() { dfu-util -d 0483:df11 -l 2>/dev/null | grep -q "Found DFU"; }

for port in /dev/serial/by-id/*FerroHAL*"$serial"*; do
    [ -e "$port" ] && printf BENCHDFU > "$port"
done
for _ in $(seq 1 100); do
    in_dfu && break
    sleep 0.3
done
if ! in_dfu; then
    echo "FAIL no DFU device: hold BOOT while plugging in the USB cable, then run this again"
    exit 1
fi

# `dfu-util` exits non-zero when the board leaves DFU before the last status.
dfu-util -d 0483:df11 -a 0 -s 0x08000000:leave -D "$elf.bin" 2>&1 | tail -2

python3 -I "$here/usb_cdc_host.py" 30 "$serial" report 2>&1 | tee "$here/logs/foxeer.log"
exit "${PIPESTATUS[0]}"
