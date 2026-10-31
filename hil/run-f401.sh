#!/usr/bin/env bash
# Runs the Nucleo-F401RE bench: UART runs A to D, with the host echo
# for USART2 running alongside, SPI and EXTI runs E to G, timer run H. Exits
# non-zero if any run failed.
#
# Needs the links described in nucleo-f401re-loopback/src/board.rs
# (J1 to J4 for the UARTs, S1 to S5 for the SPIs).
# Logs go to hil/logs/f401-<run>.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
# The F401 Nucleo's ST-LINK/V2-1 (probe 0483:374b).
port=/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_0674FF555185754867115446-if02
mkdir -p "$here/logs"

python3 -I "$here/vcp_echo.py" "$port" 115200 > "$here/logs/f401-echo.log" 2>&1 &
echo_pid=$!
trap 'kill "$echo_pid" 2>/dev/null; wait "$echo_pid" 2>/dev/null' EXIT
sleep 0.5

status=0
for run in a b c d e f g h; do
    log="$here/logs/f401-run-$run.log"
    (cd "$here/nucleo-f401re-loopback" && timeout 300 cargo run --release --bin "run-$run") > "$log" 2>&1
    code=$?
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (nucleo_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
