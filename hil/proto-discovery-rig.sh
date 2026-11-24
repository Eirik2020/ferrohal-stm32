#!/usr/bin/env bash
# PROTOTYPE (audit step 6): the STM32F4 Discovery's identity and wiring checks,
# driven by its board file (tools/rig_check.py). Through rig, which holds the
# Discovery's lock for the whole call:
#
#   rig proto-discovery-rig.sh identity [board.toml]
#   rig proto-discovery-rig.sh wiring [board.toml] [--only links|slaves|usb]
#   rig proto-discovery-rig.sh guard [board.toml] -- <bench command...>
#
# The board file defaults to hil/boards/stm32f4-discovery.toml; a planted-fault
# copy can be named instead. Nothing here flashes the board.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
mode=${1:-}
shift || true
board="$here/boards/stm32f4-discovery.toml"
case "${1:-}" in *.toml) board=$1; shift ;; esac
start=$(date +%s.%N)
python3 -I "$here/../tools/rig_check.py" "$mode" "$board" "$@"
code=$?
secs=$(python3 -I -c "import sys; print(f'{float(sys.argv[2]) - float(sys.argv[1]):.2f}')" "$start" "$(date +%s.%N)")
# The line FerroHAL's bench scripts print, which the rig's telemetry reads.
if [ "$code" -eq 0 ]; then
    echo "run proto-$mode: exit 0, 1 passed, 0 failed ($secs s)"
else
    echo "run proto-$mode: exit $code, 0 passed, 1 failed ($secs s)"
fi
exit "$code"
