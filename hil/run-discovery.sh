#!/usr/bin/env bash
# Runs the STM32F4 Discovery bench (die 0x413, `stm32f4-discovery/src/board.rs`):
# run A the die and its own UART routes over links D1 to D3, run B the F4's USB
# core on CN5 with `usb_cdc_host.py` alongside, run C the ROM bootloader, which
# `dfu-util` must then list on CN5. Exits non-zero if any run failed.
#
# Logs go to hil/logs/discovery-run-<run>.log. Arguments pick runs, say `a c`;
# none runs all. With several probes attached, set DISCOVERY_PROBE.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
crate="$here/stm32f4-discovery"
chip=STM32F407VGTx
source "$here/probe.sh"
probe_for DISCOVERY_PROBE || exit 1
mkdir -p "$here/logs"

# The serial the ROM's DFU device reports: the UID's words 0 and 2 summed, then
# word 1's upper half, in hex (measured on the Foxeer's ROM, 2026-11-14).
rom_serial() {
    probe-rs read --chip "$chip" b32 0x1FFF7A10 3 2>/dev/null | tail -1 | python3 -I -c '
import sys
w = [int(x, 16) for x in sys.stdin.read().split(":")[1].split()]
print(f"{(w[0] + w[2]) & 0xFFFFFFFF:08X}{w[1] >> 16:04X}")'
}

status=0
for run in ${*:-probe a b c}; do
    log="$here/logs/discovery-run-$run.log"
    : > "$log"
    code=0
    if [ "$run" = b ]; then
        (cd "$crate" && timeout 300 cargo build --release --bin run-b) >> "$log" 2>&1 || code=$?
        host=""
        if [ "$code" -eq 0 ]; then
            python3 -I "$here/usb_cdc_host.py" --fresh 30 f407vg > "$log.host" 2>&1 &
            host=$!
            (cd "$crate" && timeout 300 cargo run --release --bin run-b) >> "$log" 2>&1 || code=$?
            wait "$host" || code=1
            cat "$log.host" >> "$log"
            rm -f "$log.host"
        fi
    elif [ "$run" = c ]; then
        serial=$(rom_serial)
        (cd "$crate" && timeout 300 cargo build --release --bin run-c) >> "$log" 2>&1 || code=$?
        if [ "$code" -eq 0 ]; then
            (cd "$crate" && CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER="probe-rs run --no-catch-reset --chip $chip" \
                timeout 60 cargo run --release --bin run-c) >> "$log" 2>&1 &
            runner=$!
            found=""
            for _ in $(seq 1 30); do
                if dfu-util -d 0483:df11 -l 2>/dev/null | grep -q "serial=\"$serial\""; then
                    found=yes
                    break
                fi
                sleep 1
            done
            kill "$runner" 2>/dev/null
            wait "$runner" 2>/dev/null
            if [ -n "$found" ]; then
                echo "PASS rom: the ROM's DFU device $serial is listed on CN5 [RESET-004]" >> "$log"
            else
                echo "FAIL rom: no DFU device $serial on CN5 within 30 s [RESET-004]" >> "$log"
                code=1
            fi
            # Out of the ROM for good: run C's image asks for the ROM on every
            # boot, so a reset alone would bring it back, and an F4 ROM on USB
            # makes the Foxeer's DFU flashing ambiguous (same flash layout).
            # A blank chip enumerates as nothing; the next run flashes it.
            probe-rs erase --chip "$chip" >> "$log" 2>&1 || code=1
        fi
    else
        if [ "$run" = probe ]; then
            # The glue probe (plan step 5): the generated facts on the silicon,
            # through this board's probe, no firmware.
            (cd "$here/../tools/glue-gen" && timeout 300 cargo run -q --release -- probe part stm32f407vg) >> "$log" 2>&1 || code=$?
        else
            (cd "$crate" && timeout 300 cargo run --release --bin "run-$run") >> "$log" 2>&1 || code=$?
        fi
    fi
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (run_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
