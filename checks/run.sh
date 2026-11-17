#!/usr/bin/env bash
# Every check that needs no hardware, for every chip. Exits non-zero if any
# fails. The hardware benches are hil/run-f401.sh and the H7 loopback.
#
# - the formatting (rustfmt), as CI checks it;
# - clippy on the crate, per chip, with warnings as errors;
# - no reachable panic in the public API, per chip (checks/no-panic), and
#   every public function called there or listed with a reason
#   (tools/check_no_panic_coverage.py);
# - every firmware crate linking FerroHAL enables `rt` for VTOR
#   (tools/check_firmware_features.py);
# - the names of docs/design/naming.md (tools/check_names.py);
# - G5's safe API on the sources (tools/check_safe_api.py), and the
#   register hatches proven closed (checks/no-metapac compile-fail probes);
# - the pure logic's unit tests, on the host.

set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
chips=(stm32f401re stm32f411ce stm32f405rg stm32f407vg stm32f722re stm32f745vg stm32f767zi stm32h743vi stm32h753zi)
# Every other package of those lines: the same hand facts, its own generated
# glue, so it gets clippy and the unit tests, not the full set. A flash-size
# variant mounts its package's glue and is not listed
# (tools/glue-gen/src/registry.rs).
packages=(
    stm32f401ce stm32f401ve stm32f411re stm32f411ve
    stm32f405og stm32f405vg stm32f405zg stm32f407zg stm32f407ig
    stm32f722ve stm32f722ze stm32f722ie stm32f745zg stm32f745ig
    stm32f767vi stm32f767ii stm32f767bi stm32f767ni
    stm32h743zi stm32h743ii stm32h743bi stm32h743ai stm32h743xi
    stm32h753vi stm32h753ii stm32h753bi stm32h753ai stm32h753xi
)
# The F7 has no UART driver until the UART redesign lands, and the no-panic
# program is built around one, so the F7 joins it with its UART (the F7 plan).
no_panic_skip=(stm32f722re stm32f745vg stm32f767zi)
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
    # A broken intra-doc link, or one into a private item, is a doc that
    # points nowhere for the crate's users.
    step "rustdoc ($chip)" \
        env RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit,usb"
    # From its own directory: cargo reads .cargo/config.toml from where it
    # runs, and without that package's link.x the linker can discard
    # everything, panics included, and pass for nothing. A linker warning
    # fails the step, and the build that raised it is kept to be printed.
    if [[ " ${no_panic_skip[*]} " == *" $chip "* ]]; then
        echo "skip  no reachable panic ($chip): no UART driver yet (docs/plans/2026-11-17-f411-f722-f745.md)"
        continue
    fi
    step "no reachable panic ($chip)" \
        bash -c 'cd "$1" || exit 1; out=$(cargo build --release --features "$2" 2>&1); if grep -q "warning: linker stderr" <<< "$out"; then printf "%s\n" "$out"; exit 1; fi; cargo build --release --features "$2"' _ "$root/checks/no-panic" "$chip"
done
for part in "${packages[@]}"; do
    step "clippy ($part)" \
        cargo clippy --manifest-path "$root/Cargo.toml" --features "$part,rt,defmt,fugit,usb" -- -D warnings
done
step "no-panic calls every public function" python3 -I "$root/tools/check_no_panic_coverage.py"
step "firmware crates enable rt" python3 -I "$root/tools/check_firmware_features.py"
step "safe api (G5)" python3 -I "$root/tools/check_safe_api.py"
step "names (ADR-003)" python3 -I "$root/tools/check_names.py"
step "no-metapac probes" bash "$root/checks/no-metapac/run.sh"
step "evidence recorder tests" python3 -I "$root/tools/test_record_evidence.py"
# The requirements against the code (a renamed test a requirement names fails here):
# reqtool from $REQTOOL, or the sibling checkout CI pins (tools/record_evidence.py too).
# CI has no sibling checkout here: its Requirements job installs reqtool and runs this.
reqtool="${REQTOOL:-$root/../reqtool/target/release/reqtool}"
if [ -x "$reqtool" ]; then
    step "requirements valid (reqtool validate)" bash -c 'cd "$1" && "$2" validate' _ "$root" "$reqtool"
else
    echo "skip  requirements valid: no reqtool at $reqtool (set REQTOOL)"
fi
step "docs index up to date" python3 -I "$root/tools/docs_index.py" --check
step "doc size limits" python3 -I "$root/tools/check_docs.py"
host=$(rustc -vV | sed -n 's/^host: //p')
for chip in "${chips[@]}" "${packages[@]}"; do
    step "host unit tests ($chip)" \
        cargo test --lib --manifest-path "$root/Cargo.toml" --features "$chip" --target "$host"
done
exit "$status"
