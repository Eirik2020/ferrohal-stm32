#!/usr/bin/env bash
# Runs the Nucleo-F401RE bench: UART runs A to D, with the host echo for
# USART2 running alongside, SPI and EXTI runs E to G, timer runs H and I,
# watchdog run J, polled SPI run K, bootloader request run L, ADC run M, timer trigger run N, waveform loopback runs O and P, I2C run Q (no slave fitted), the SpeedyBee's four motor lanes run R, I2C run S against the Pico 1 simulator (I2C3 on PA8/PC9; hil/WIRING.md has the wiring), run V, the constructor refusals, run W, the injected faults, and run X, the lane switch's cycles (built with `profile-listen`). Exits non-zero if any run failed.
#
# Needs the links described in nucleo-f401re-loopback/src/board.rs
# (J1 to J4 for the UARTs, S1 to S5 for the SPIs).
# Logs go to hil/logs/f401-<run>.log. Arguments pick runs, say `e f g`; none runs all.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
# A run letter's binary, from the bench's alias table (API review B1).
bin_of() { awk -v r="$1" '$1 == r { print $2 }' "$here/nucleo-f401re-loopback/runs.tsv"; }
source "$here/probe.sh"
probe_for F401_PROBE 0x433 || exit 1
# The host echo runs over this probe's own virtual COM port.
port="${F401_VCP:-/dev/serial/by-id/usb-STMicroelectronics_STM32_STLink_${PROBE_RS_PROBE#*:*:}-if02}"
mkdir -p "$here/logs"

# Every image first, in one cargo call, which compiles them in parallel: each
# run's own `cargo run` then only finds it built. A bin that fails to build here
# still fails its own run below, with the compiler's message in that run's log.
# Bins behind a feature (`required-features`) are left to their runs.
case " ${*:-all} " in
    " probe ") ;;
    *) if (cd "$here/nucleo-f401re-loopback" && timeout 600 cargo build --release --bins) > "$here/logs/f401-build.log" 2>&1; then
           echo "build: every image built ($(grep -oE 'in [0-9.]+(s|m [0-9]+s)$' "$here/logs/f401-build.log" | tail -1))"
       else
           echo "build: some images failed; their runs will say which (hil/logs/f401-build.log)"
       fi ;;
esac

python3 -I "$here/vcp_echo.py" "$port" 115200 > "$here/logs/f401-echo.log" 2>&1 &
echo_pid=$!
trap 'kill "$echo_pid" 2>/dev/null; wait "$echo_pid" 2>/dev/null' EXIT
sleep 0.5

status=0
for run in ${*:-probe a b c d e f g h i j k l m n o p q r s v w x}; do
    log="$here/logs/f401-run-$run.log"
    # The watchdog and bootloader runs reset the chip on purpose: the debugger must not
    # catch that reset, or the second boot never runs. Run V's own watchdog
    # does the same at the end of its first boot.
    runner=()
    if [ "$run" = j ] || [ "$run" = l ] || [ "$run" = v ]; then
        runner=(env "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUNNER=probe-rs run --no-catch-reset --chip STM32F401RE")
    fi
    if [ "$run" = probe ]; then
        # The glue probe (plan step 5): the generated facts on the silicon,
        # through this board's probe, no firmware.
        (cd "$here/../tools/glue-gen" && timeout 300 cargo run -q --release -- probe part stm32f401re) > "$log" 2>&1
    else
        # Run X times the lane switch through FerroHAL's cycle stamps.
        features=()
        [ "$run" = x ] && features=(--features profile-listen)
        if [ "${#features[@]}" -gt 0 ]; then echo "build features: ${features[1]}"; else echo "build features: none"; fi > "$log"
        (cd "$here/nucleo-f401re-loopback" && timeout 300 "${runner[@]}" cargo run --release "${features[@]}" --bin "$(bin_of "$run")") >> "$log" 2>&1
    fi
    code=$?
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (nucleo_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
