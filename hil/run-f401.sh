#!/usr/bin/env bash
# Runs the Nucleo-F401RE bench: UART runs A to D, with the host echo for
# USART2 running alongside, SPI and EXTI runs E to G, timer runs H and I,
# watchdog run J, polled SPI run K, bootloader request run L, ADC run M, timer trigger run N, waveform loopback runs O and P, I2C run Q (no slave fitted), the SpeedyBee's four motor lanes run R, and I2C run S against the Pico 1 simulator (I2C3 on PA8/PC9; hil/WIRING.md has the wiring). Exits non-zero if any run failed.
#
# Needs the links described in nucleo-f401re-loopback/src/board.rs
# (J1 to J4 for the UARTs, S1 to S5 for the SPIs).
# Logs go to hil/logs/f401-<run>.log. Arguments pick runs, say `e f g`; none runs all.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
source "$here/probe.sh"
probe_for F401_PROBE || exit 1
# The host echo runs over this probe's own virtual COM port.
port="${F401_VCP:-/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_${PROBE_RS_PROBE#*:*:}-if02}"
mkdir -p "$here/logs"

python3 -I "$here/vcp_echo.py" "$port" 115200 > "$here/logs/f401-echo.log" 2>&1 &
echo_pid=$!
trap 'kill "$echo_pid" 2>/dev/null; wait "$echo_pid" 2>/dev/null' EXIT
sleep 0.5

status=0
for run in ${*:-a b c d e f g h i j k l m n o p q r s u}; do
    log="$here/logs/f401-run-$run.log"
    # The watchdog and bootloader runs reset the chip on purpose: the debugger must not
    # catch that reset, or the second boot never runs.
    runner=()
    if [ "$run" = j ] || [ "$run" = l ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip STM32F401RE")
    fi
    (cd "$here/nucleo-f401re-loopback" && timeout 300 "${runner[@]}" cargo run --release --bin "run-$run") > "$log" 2>&1
    code=$?
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (nucleo_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
