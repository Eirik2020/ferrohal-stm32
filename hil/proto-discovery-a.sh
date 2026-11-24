#!/usr/bin/env bash
# PROTOTYPE (audit step 6): candidate A (embedded-test) on the STM32F4
# Discovery. Through rig:
#
#   rig proto-discovery-a.sh <die|route> [features|-] [probe-rs test args...]
#
# Checks the board's identity (hil/boards/stm32f4-discovery.toml), builds the
# test image with the features named (hang-clocks, hang-check), and runs it
# with probe-rs, which runs each test after a reset and holds its timeout;
# tools/proto_harness_a.py times the lines. Never requirement evidence.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
board="$here/boards/stm32f4-discovery.toml"
image=${1:?image: die or route}
features=${2:--}
shift $(( $# < 2 ? $# : 2 ))
name="a-$image"
[ "$features" != - ] && name="$name-$features"
[ $# -gt 0 ] && name="$name-sel"
case "$image" in
    die) crate="$here/proto-discovery-a" ;;
    route) crate="$here/proto-discovery-a-rtic" ;;
    *) echo "image: die or route"; exit 2 ;;
esac
python3 -I "$here/../tools/rig_check.py" identity "$board" || { echo "run $name: exit 3, 0 passed, 1 failed"; exit 3; }
args=(--release --test "$image" --message-format=json)
[ "$features" != - ] && args+=(--features "$features")
t0=$(date +%s.%N)
mkdir -p "$here/logs"
if ! (cd "$crate" && timeout 600 cargo build "${args[@]}") > "$here/logs/proto-$name-build.json" 2> "$here/logs/proto-$name-build.log"; then
    tail -20 "$here/logs/proto-$name-build.log"
    echo "run $name: exit 2, 0 passed, 1 failed"
    exit 2
fi
elf=$(python3 -I -c '
import json, sys
for line in open(sys.argv[1]):
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == sys.argv[2]:
        print(m["executable"])' "$here/logs/proto-$name-build.json" "$image" | tail -1)
echo "build: $(python3 -I -c "import sys; print(f'{float(sys.argv[2]) - float(sys.argv[1]):.2f}')" "$t0" "$(date +%s.%N)") s, $elf"
exec python3 -I "$here/../tools/proto_harness_a.py" "$board" "$elf" "$name" "$@"
