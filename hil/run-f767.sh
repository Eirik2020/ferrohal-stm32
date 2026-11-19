#!/usr/bin/env bash
# Runs the Nucleo-F767ZI bench (die 0x451, `nucleo-f767zi/src/board.rs`): the
# glue probe, run A the clock tree with over-drive and every timer, run B the
# watchdog across the reset it causes, run C the I2C master with no slave
# fitted, run D the interrupt-driven I2C master against the second Pico
# simulator on run C's pins. Only run D needs the Pico. Exits non-zero if
# any run failed.
#
# Logs go to hil/logs/f767-run-<run>.log. Arguments pick runs, say `a c`; none
# runs all. With several probes attached, set F767_PROBE.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
crate="$here/nucleo-f767zi"
chip=STM32F767ZITx
source "$here/probe.sh"
probe_for F767_PROBE 0x451 || exit 1
mkdir -p "$here/logs"

status=0
for run in ${*:-probe a b c d}; do
    log="$here/logs/f767-run-$run.log"
    runner=()
    # The watchdog run resets the chip on purpose: the debugger must not catch
    # that reset, or the second boot never runs.
    if [ "$run" = b ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip $chip")
    fi
    if [ "$run" = probe ]; then
        # The glue probe: the generated facts on the silicon, no firmware.
        (cd "$here/../tools/glue-gen" && timeout 300 cargo run -q --release -- probe part stm32f767zi) > "$log" 2>&1
    else
        (cd "$crate" && timeout 300 "${runner[@]}" cargo run --release --bin "run-$run") > "$log" 2>&1
    fi
    code=$?
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (nucleo_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
