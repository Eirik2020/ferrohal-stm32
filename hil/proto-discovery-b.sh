#!/usr/bin/env bash
# PROTOTYPE (audit step 6): candidate B on the STM32F4 Discovery. Through rig:
#
#   rig proto-discovery-b.sh <die|route> [features|-] [select words...]
#
# Checks the board's identity (hil/boards/stm32f4-discovery.toml), builds the
# image with the features named (hang-clocks, hang-check), and runs it under
# tools/proto_harness_b.py, which holds each announced stage's limit.
# B_PROBE_RS_ARGS passes one flag to probe-rs run (--no-catch-reset). Never
# requirement evidence.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
board="$here/boards/stm32f4-discovery.toml"
image=${1:?image: die or route}
features=${2:--}
shift $(( $# < 2 ? $# : 2 ))
select="$*"
name="b-$image"
[ "$features" != - ] && name="$name-$features"
[ -n "$select" ] && name="$name-sel"
python3 -I "$here/../tools/rig_check.py" identity "$board" || { echo "run $name: exit 3, 0 passed, 1 failed"; exit 3; }
crate="$here/proto-discovery-b"
args=(--release --bin "$image")
[ "$features" != - ] && args+=(--features "$features")
t0=$(date +%s.%N)
if ! (cd "$crate" && timeout 600 cargo build "${args[@]}") > "$here/logs/proto-$name-build.log" 2>&1; then
    tail -20 "$here/logs/proto-$name-build.log"
    echo "run $name: exit 2, 0 passed, 1 failed"
    exit 2
fi
echo "build: $(python3 -I -c "import sys; print(f'{float(sys.argv[2]) - float(sys.argv[1]):.2f}')" "$t0" "$(date +%s.%N)") s"
exec python3 -I "$here/../tools/proto_harness_b.py" "$board" \
    "$crate/target/thumbv7em-none-eabihf/release/$image" --select "$select" --name "$name" \
    ${B_PROBE_RS_ARGS:+--probe-rs "$B_PROBE_RS_ARGS"}
