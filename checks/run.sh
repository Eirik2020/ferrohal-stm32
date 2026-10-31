#!/usr/bin/env bash
# Every check that needs no hardware, for every chip. Exits non-zero if any
# fails. The hardware benches are hil/run-f401.sh and the H7 loopback.
#
# - clippy on the crate, per chip, with warnings as errors;
# - no reachable panic in the public API, per chip (checks/no-panic);
# - the chip glue against stm32-metapac's metadata (tools/check_glue.py);
# - the pure logic's unit tests, on the host.

set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
chips=(stm32f401re stm32f405rg stm32h743vi stm32h753zi)
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
        cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit" -- -D warnings
    # From its own directory: cargo reads .cargo/config.toml from where it
    # runs, and without that package's link.x the linker can discard
    # everything, panics included, and pass for nothing.
    step "no reachable panic ($chip)" \
        bash -c 'cd "$1" && cargo build --release --features "$2" 2>&1 | grep -q "warning: linker stderr" && exit 1; cd "$1" && cargo build --release --features "$2"' _ "$root/checks/no-panic" "$chip"
done
step "glue matches stm32-metapac metadata" python3 -I "$root/tools/check_glue.py"
host=$(rustc -vV | sed -n 's/^host: //p')
for chip in "${chips[@]}"; do
    step "host unit tests ($chip)" \
        cargo test --lib --manifest-path "$root/Cargo.toml" --features "$chip" --target "$host"
done
exit "$status"
