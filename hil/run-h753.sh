#!/usr/bin/env bash
# Runs the Nucleo-H753ZI bench: UART runs A to E, SPI and EXTI runs F to H,
# timer runs I and J, watchdog run K,
# polled SPI run L, USB CDC run M (over CN13, which stays connected), bootloader request
# run N, ADC run O, timer trigger run P, waveform loopback run Q, I2C run R
# (I2C3 on PA8/PC9, no slave fitted, no links needed), I2C run S against
# the Pico 1 simulator (I2C4 on PD12/PD13; hil/WIRING.md has the wiring), and
# constructor-refusal run U (needs Z2, S1 and S3; no USB check: this chip's
# constructor starts its own 48 MHz source and cannot be made to refuse),
# the fault and cache runs W and X, and run Y, USART3 into UART4 over Z6.
# Every other UART and SPI looped back to itself. Exits non-zero if any run failed.
#
# Needs the Zio links described in nucleo-h753zi-loopback/src/board.rs.
# Logs go to hil/logs/h753-<run>.log. Arguments pick runs, say `f g h`; none runs all.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
# A run letter's binary, from the bench's alias table (API review B1).
bin_of() { awk -v r="$1" '$1 == r { print $2 }' "$here/nucleo-h753zi-loopback/runs.tsv"; }
mkdir -p "$here/logs"
source "$here/probe.sh"
probe_for H753_PROBE 0x450 || exit 1

# Every image first, in one cargo call, which compiles them in parallel: each
# run's own `cargo run` then only finds it built. A bin that fails to build here
# still fails its own run below, with the compiler's message in that run's log.
# Bins behind a feature (`required-features`) are left to their runs.
case " ${*:-all} " in
    " probe ") ;;
    *) if (cd "$here/nucleo-h753zi-loopback" && timeout 600 cargo build --release --bins) > "$here/logs/h753-build.log" 2>&1; then
           echo "build: every image built ($(grep -oE 'in [0-9.]+(s|m [0-9]+s)$' "$here/logs/h753-build.log" | tail -1))"
       else
           echo "build: some images failed; their runs will say which (hil/logs/h753-build.log)"
       fi ;;
esac

status=0
# Run M still runs when asked for (`./run-h753.sh m`), with the cable
# moved back to CN13; it is off the defaults below while the cable
# powers the Pico.
# The H7 ROM's DFU serial starts with the die's DBGMCU_IDC, in hex, then four
# digits of its own (this H753: 200364500070, the Lucid: 200364500000; measured
# 2026-11-15), unlike the F4 ROM's, derived from the UID.
idcode() {
    probe-rs read --chip STM32H753ZITx b32 0x5C001000 1 2>/dev/null | tail -1 | awk '{print toupper($2)}'
}
dfu_serials() {
    dfu-util -d 0483:df11 -l 2>/dev/null | sed -n 's/.*serial="\([^"]*\)".*/\1/p' | sort -u
}

for run in ${*:-probe a b c d e f g h i j k l m n o p q r s t u w x y z}; do
    log="$here/logs/h753-run-$run.log"
    : > "$log"
    # The watchdog and bootloader runs reset the chip on purpose: the debugger must not
    # catch that reset, or the second boot never runs.
    runner=()
    if [ "$run" = k ] || [ "$run" = n ] || [ "$run" = u ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip STM32H753ZI")
    fi
    # The USB run needs its host side alongside: build first, so the host's
    # wait for the port is not spent compiling. The host waits for the port
    # the previous image left to go, so a rerun cannot open it while this
    # run is being flashed.
    host=""
    code=0
    if [ "$run" = m ]; then
        (cd "$here/nucleo-h753zi-loopback" && timeout 300 cargo build --release --bin "$(bin_of m)") >> "$log" 2>&1
        code=$?
        if [ "$code" -eq 0 ]; then
            python3 -I "$here/usb_cdc_host.py" --fresh 30 h753zi > "$log.host" 2>&1 &
            host=$!
        fi
    fi
    if [ "$code" -eq 0 ] && [ "$run" = z ]; then
        # The ROM bootloader (RESET-004): the image asks for it on every boot,
        # so the script checks for its DFU device, then erases the chip.
        prefix=$(idcode)
        before=$(dfu_serials)
        (cd "$here/nucleo-h753zi-loopback" && timeout 300 cargo build --release --bin "$(bin_of z)") >> "$log" 2>&1 || code=$?
        if [ "$code" -eq 0 ]; then
            (cd "$here/nucleo-h753zi-loopback" && CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER="probe-rs run --no-catch-reset --chip STM32H753ZI" \
                timeout 60 cargo run --release --bin "$(bin_of z)") >> "$log" 2>&1 &
            runner_pid=$!
            found=""
            for _ in $(seq 1 30); do
                found=$(dfu_serials | grep "^$prefix" | grep -vxF -f <(printf '%s\n' "$before") | head -1)
                [ -n "$found" ] && break
                sleep 1
            done
            kill "$runner_pid" 2>/dev/null
            wait "$runner_pid" 2>/dev/null
            if [ -n "$found" ]; then
                echo "PASS rom: a new DFU device $found, this die's IDCODE $prefix, is listed on CN13 [RESET-004]" >> "$log"
            else
                echo "FAIL rom: no new DFU device with IDCODE $prefix on CN13 within 30 s [RESET-004]; listed:" >> "$log"
                dfu_serials >> "$log"
                code=1
            fi
            probe-rs erase --chip STM32H753ZITx >> "$log" 2>&1 || code=1
        fi
    elif [ "$code" -eq 0 ]; then
        if [ "$run" = probe ]; then
            # The glue probe (plan step 5): the generated facts on the silicon,
            # through this board's probe, no firmware.
            (cd "$here/../tools/glue-gen" && timeout 300 cargo run -q --release -- probe part stm32h753zi) >> "$log" 2>&1
        else
            features=()
            # Run T alone builds with the library's bench fault injection.
            [ "$run" = t ] && features=(--features fault-injection)
            # For tools/record_evidence.py: what this image was built with.
            if [ "${#features[@]}" -gt 0 ]; then echo "build features: ${features[1]}"; else echo "build features: none"; fi >> "$log"
            (cd "$here/nucleo-h753zi-loopback" && timeout 300 "${runner[@]}" cargo run --release "${features[@]}" --bin "$(bin_of "$run")") >> "$log" 2>&1
        fi
        code=$?
    fi
    if [ -n "$host" ]; then
        wait "$host" || code=1
        cat "$log.host" >> "$log"
        rm -f "$log.host"
    fi
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (run_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
