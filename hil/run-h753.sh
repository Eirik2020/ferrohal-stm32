#!/usr/bin/env bash
# Runs the Nucleo-H753ZI bench: UART runs A to E, SPI and EXTI runs F to H,
# timer runs I and J, watchdog run K,
# polled SPI run L, USB CDC run M (needs a data cable from CN13 to the host). Every UART and SPI looped back to
# itself. Exits non-zero if any run failed.
#
# Needs the Zio links described in nucleo-h753zi-loopback/src/board.rs.
# Logs go to hil/logs/h753-<run>.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/logs"

status=0
for run in a b c d e f g h i j k l m; do
    log="$here/logs/h753-run-$run.log"
    # The watchdog run resets the chip on purpose: the debugger must not
    # catch that reset, or the second boot never runs.
    runner=()
    if [ "$run" = k ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip STM32H753ZI --probe 0483:374e")
    fi
    # The USB run needs its host side alongside: the host checks every byte.
    host=""
    if [ "$run" = m ]; then
        python3 -I "$here/usb_cdc_host.py" 30 > "$log.host" 2>&1 &
        host=$!
    fi
    (cd "$here/nucleo-h753zi-loopback" && timeout 300 "${runner[@]}" cargo run --release --bin "run-$run") > "$log" 2>&1
    code=$?
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
