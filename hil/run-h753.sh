#!/usr/bin/env bash
# Runs the Nucleo-H753ZI bench: UART runs A to E, SPI and EXTI runs F to H,
# timer runs I and J, watchdog run K,
# polled SPI run L, USB CDC run M (off the default list: its CN13 cable now
# powers the Pico 1 — move it back to run `m` by hand), bootloader request
# run N, ADC run O, timer trigger run P, waveform loopback run Q, I2C run R
# (I2C3 on PA8/PC9, no slave fitted, no links needed), I2C run S against
# the Pico 1 simulator (I2C4 on PD12/PD13; hil/WIRING.md has the wiring), and
# constructor-refusal run U (needs Z2, S1 and S3; no USB check: this chip's
# constructor starts its own 48 MHz source and cannot be made to refuse).
# Every UART and SPI looped back to itself. Exits non-zero if any run failed.
#
# Needs the Zio links described in nucleo-h753zi-loopback/src/board.rs.
# Logs go to hil/logs/h753-<run>.log. Arguments pick runs, say `f g h`; none runs all.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/logs"
source "$here/probe.sh"
probe_for H753_PROBE || exit 1

status=0
# Run M still runs when asked for (`./run-h753.sh m`), with the cable
# moved back to CN13; it is off the defaults below while the cable
# powers the Pico.
for run in ${*:-a b c d e f g h i j k l n o p q r s t u w}; do
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
        (cd "$here/nucleo-h753zi-loopback" && timeout 300 cargo build --release --bin run-m) >> "$log" 2>&1
        code=$?
        if [ "$code" -eq 0 ]; then
            python3 -I "$here/usb_cdc_host.py" --fresh 30 h753zi > "$log.host" 2>&1 &
            host=$!
        fi
    fi
    if [ "$code" -eq 0 ]; then
        (cd "$here/nucleo-h753zi-loopback" && timeout 300 "${runner[@]}" cargo run --release --bin "run-$run") >> "$log" 2>&1
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
