#!/usr/bin/env bash
# Every check that needs no hardware, for every chip. Exits non-zero if any
# fails. The hardware benches are hil/run-f401.sh and the H7 loopback.
#
# - the formatting (rustfmt), as CI checks it;
# - clippy on the crate, per chip, with warnings as errors;
# - no reachable panic in the public API, per chip (checks/no-panic);
# - the chip glue against stm32-metapac's metadata (tools/check_glue.py);
# - every firmware crate linking FerroHAL enables `rt` for VTOR
#   (tools/check_firmware_features.py);
# - the names of docs/design/naming.md (tools/check_names.py);
# - G5's safe API on the sources (tools/check_safe_api.py), and the
#   register hatches proven closed (checks/no-metapac compile-fail probes);
# - the pure logic's unit tests, on the host.

set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
chips=(stm32f401re stm32f405rg stm32h743vi stm32h753zi)
status=0

# A passing step prints only its pass line. A failing one prints its FAIL line
# and then, on stderr, everything the step wrote, so the cause is in the log.
step() {
    local name=$1
    shift
    local output
    if output=$("$@" 2>&1); then
        echo "pass  $name"
    else
        echo "FAIL  $name"
        printf '%s\n' "$output" >&2
        status=1
    fi
}

step "rustfmt" cargo fmt --manifest-path "$root/Cargo.toml" --check
for chip in "${chips[@]}"; do
    step "clippy ($chip)" \
        cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit,usb" -- -D warnings
    step "clippy without usb ($chip)" \
        cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit" -- -D warnings
    # From its own directory: cargo reads .cargo/config.toml from where it
    # runs, and without that package's link.x the linker can discard
    # everything, panics included, and pass for nothing. A linker warning
    # fails the step, and the build that raised it is kept to be printed.
    step "no reachable panic ($chip)" \
        bash -c 'cd "$1" || exit 1; out=$(cargo build --release --features "$2" 2>&1); if grep -q "warning: linker stderr" <<< "$out"; then printf "%s\n" "$out"; exit 1; fi; cargo build --release --features "$2"' _ "$root/checks/no-panic" "$chip"
done
step "glue matches stm32-metapac metadata" python3 -I "$root/tools/check_glue.py"
step "firmware crates enable rt" python3 -I "$root/tools/check_firmware_features.py"
step "safe api (G5)" python3 -I "$root/tools/check_safe_api.py"
step "names (ADR-003)" python3 -I "$root/tools/check_names.py"
step "no-metapac probes" bash "$root/checks/no-metapac/run.sh"
step "docs index up to date" python3 -I "$root/tools/docs_index.py" --check
step "doc size limits" python3 -I "$root/tools/check_docs.py"
host=$(rustc -vV | sed -n 's/^host: //p')
for chip in "${chips[@]}"; do
    step "host unit tests ($chip)" \
        cargo test --lib --manifest-path "$root/Cargo.toml" --features "$chip" --target "$host"
done
exit "$status"
