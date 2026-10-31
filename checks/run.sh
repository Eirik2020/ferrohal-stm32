#!/usr/bin/env bash
# Every check that needs no hardware, for every chip. Exits non-zero if any
# fails. The hardware benches are hil/run-f401.sh and the H7 loopback.
#
# - clippy on the crate, per chip, with warnings as errors;
# - no reachable panic in the public API, per chip (checks/no-panic);
# - the chip glue against stm32-metapac's metadata (tools/check_glue.py).

set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
chips=(stm32f401re stm32h753zi)
status=0

step() {
    local name=$1
    shift
    if "$@" > /dev/null 2>&1; then
        echo "pass  $name"
    else
        echo "FAIL  $name"
        status=1
    fi
}

for chip in "${chips[@]}"; do
    step "clippy ($chip)" \
        cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt" -- -D warnings
    step "no reachable panic ($chip)" \
        cargo build --release --manifest-path "$root/checks/no-panic/Cargo.toml" --features "$chip"
done
step "glue matches stm32-metapac metadata" python3 -I "$root/tools/check_glue.py"
exit "$status"
