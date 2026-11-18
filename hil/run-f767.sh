#!/usr/bin/env bash
# Runs the Nucleo-F767ZI bench (die 0x451, `nucleo-f767zi/src/board.rs`): the
# glue probe, run A the clock tree with over-drive and every timer, run B the
# watchdog across the reset it causes, run C the I2C master with no slave
# fitted; none of those needs a link. Run D five UARTs over links F1 to F4 and F6,
# run E UART4 over F5 and USART3 through the ST-LINK's virtual COM port, with
# the host echo running alongside, run F SPI1, SPI2, SPI4 and SPI5 over links S1
# to S4. Exits non-zero if any run failed.
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
# Run E's host echo runs over this probe's own virtual COM port.
port="${F767_VCP:-/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_${PROBE_RS_PROBE#*:*:}-if02}"

status=0
for run in ${*:-probe a b c d e f}; do
    log="$here/logs/f767-run-$run.log"
    runner=()
    # The watchdog run resets the chip on purpose: the debugger must not catch
    # that reset, or the second boot never runs.
    if [ "$run" = b ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip $chip")
    fi
    echo_pid=""
    if [ "$run" = e ]; then
        python3 -I "$here/vcp_echo.py" "$port" 115200 > "$here/logs/f767-echo.log" 2>&1 &
        echo_pid=$!
        sleep 0.5
    fi
    if [ "$run" = probe ]; then
        # The glue probe: the generated facts on the silicon, no firmware.
        (cd "$here/../tools/glue-gen" && timeout 300 cargo run -q --release -- probe part stm32f767zi) > "$log" 2>&1
    else
        (cd "$crate" && timeout 300 "${runner[@]}" cargo run --release --bin "run-$run") > "$log" 2>&1
    fi
    code=$?
    if [ -n "$echo_pid" ]; then
        kill "$echo_pid" 2>/dev/null
        wait "$echo_pid" 2>/dev/null
    fi
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (nucleo_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
