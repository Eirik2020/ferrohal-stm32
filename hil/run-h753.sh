#!/usr/bin/env bash
# Runs the Nucleo-H753ZI UART bench: runs A to D, every UART looped back to
# itself. Exits non-zero if any run failed.
#
# Needs the Zio links described in nucleo-h753zi-loopback/src/board.rs.
# Logs go to hil/logs/h753-<run>.log.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/logs"

status=0
for run in a b c d e f g h; do
    log="$here/logs/h753-run-$run.log"
    (cd "$here/nucleo-h753zi-loopback" && timeout 300 cargo run --release --bin "run-$run") > "$log" 2>&1
    code=$?
    pass=$(grep -c "PASS" "$log")
    fail=$(grep -c "FAIL" "$log")
    echo "run $run: exit $code, $pass passed, $fail failed"
    grep -E "FAIL|WARN|panicked" "$log" | sed 's/ (run_.*//'
    [ "$code" -eq 0 ] || status=1
done
exit "$status"
