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
# - the pure logic's unit tests, on the host;
# - every crate under examples/ built, linted and formatted, so a rename
#   that would break a user's starting point fails here.
#
# The per-chip steps run as one job per chip, side by side, each in its own
# target directory (target/checks/<chip>) so cargo's build lock does not
# serialise them; the chip-independent steps run meanwhile. The log keeps the
# order a serial run printed. CHECKS_JOBS=1 runs the chips one at a time.
# It needs no board, so it can run while the benches do: start it beside
# them rather than after, and the merge gate waits for the longer of the two.

set -u
root="$(cd "$(dirname "$0")/.." && pwd)"
chips=(stm32f401re stm32f410rb stm32f411ce stm32f405rg stm32f407vg stm32f427vi stm32f429zi stm32f412zg stm32f413zh stm32f446re stm32f722re stm32f745vg stm32f767zi stm32h743vi stm32h753zi)
# Every other package of those lines: the same hand facts, its own generated
# glue, so it gets clippy, not the full set. A flash-size variant mounts its
# package's glue and is not listed (tools/glue-gen/src/registry.rs). They run
# as one more job, in its own target directory.
packages=(
    stm32f410tb stm32f410cb
    stm32f401ce stm32f401ve stm32f411re stm32f411ve
    stm32f405og stm32f405vg stm32f405zg stm32f407zg stm32f407ig
    stm32f427zi stm32f427ii stm32f427ai
    stm32f429vi stm32f429ii stm32f429ai stm32f429bi stm32f429ni
    stm32f412cg stm32f412rg stm32f412vg
    stm32f413ch stm32f413mh stm32f413rh stm32f413vh
    stm32f446me stm32f446ve stm32f446ze
    stm32f722ve stm32f722ze stm32f722ie stm32f745zg stm32f745ig
    stm32f765vi stm32f765zi stm32f765ii stm32f765bi stm32f765ni
    stm32f767vi stm32f767ii stm32f767bi stm32f767ni
    stm32f768ai stm32f769ai stm32f769bi stm32f769ii stm32f769ni
    stm32f777vi stm32f777zi stm32f777ii stm32f777bi stm32f777ni stm32f778ai
    stm32f779ai stm32f779bi stm32f779ii stm32f779ni
    stm32h743zi stm32h743ii stm32h743bi stm32h743ai stm32h743xi
    stm32h750vb stm32h750zb stm32h750ib stm32h750xb
    stm32h753vi stm32h753ii stm32h753bi stm32h753ai stm32h753xi
)
# `,usb` for a part with a USB port; the F410 has none (RM0401), and the
# library refuses the feature there.
usb() {
    case $1 in
        stm32f410*) ;;
        *) echo ",usb" ;;
    esac
}
status=0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

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

# One chip's steps. Its lines go to $work/<chip>.lint (before the shared
# steps in the log) and $work/<chip>.host (the host tests, after them), its
# failures' output to $work/<chip>.err, and a failure leaves $work/<chip>.fail;
# a subshell, so its status stays its own.
chip_job() (
    chip=$1
    target="$root/target/checks/$1"
    status=0
    {
        step "clippy ($chip)" \
            env CARGO_TARGET_DIR="$target" cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit$(usb "$chip")" -- -D warnings
        step "clippy without usb ($chip)" \
            env CARGO_TARGET_DIR="$target" cargo clippy --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit" -- -D warnings
        # A broken intra-doc link, or one into a private item, is a doc that
        # points nowhere for the crate's users.
        step "rustdoc ($chip)" \
            env CARGO_TARGET_DIR="$target" RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --manifest-path "$root/Cargo.toml" --features "$chip,rt,defmt,fugit$(usb "$chip")"
        # From its own directory: cargo reads .cargo/config.toml from where it
        # runs, and without that package's link.x the linker can discard
        # everything, panics included, and pass for nothing. A linker warning
        # fails the step, and the build that raised it is kept to be printed.
        step "no reachable panic ($chip)" \
            env CARGO_TARGET_DIR="$target/no-panic" bash -c 'cd "$1" || exit 1; out=$(cargo build --release --features "$2" 2>&1); if grep -q "warning: linker stderr" <<< "$out"; then printf "%s\n" "$out"; exit 1; fi; cargo build --release --features "$2"' _ "$root/checks/no-panic" "$chip"
    } > "$work/$chip.lint" 2>> "$work/$chip.err"
    step "host unit tests ($chip)" \
        env CARGO_TARGET_DIR="$target" cargo test --lib --manifest-path "$root/Cargo.toml" --features "$chip" --target "$host" \
        > "$work/$chip.host" 2>> "$work/$chip.err"
    [ "$status" -eq 0 ] || : > "$work/$chip.fail"
)

host=$(rustc -vV | sed -n 's/^host: //p')
step "rustfmt" cargo fmt --manifest-path "$root/Cargo.toml" --check
jobs=()
for chip in "${chips[@]}"; do
    if [ "${CHECKS_JOBS:-0}" = 1 ]; then
        chip_job "$chip"
    else
        chip_job "$chip" &
        jobs+=($!)
    fi
done
packages_job() (
    status=0
    for part in "${packages[@]}"; do
        step "clippy ($part)" \
            env CARGO_TARGET_DIR="$root/target/checks/packages" cargo clippy --manifest-path "$root/Cargo.toml" --features "$part,rt,defmt,fugit$(usb "$part")" -- -D warnings
    done > "$work/packages.lint" 2>> "$work/packages.err"
    [ "$status" -eq 0 ] || : > "$work/packages.fail"
)
if [ "${CHECKS_JOBS:-0}" = 1 ]; then
    packages_job
else
    packages_job &
    jobs+=($!)
fi
# The chip-independent steps, while the chips build.
shared=$(
    step "no-panic calls every public function" python3 -I "$root/tools/check_no_panic_coverage.py"
    step "firmware crates enable rt" python3 -I "$root/tools/check_firmware_features.py"
    step "safe api (G5)" python3 -I "$root/tools/check_safe_api.py"
    step "names (ADR-003)" python3 -I "$root/tools/check_names.py"
    step "decision label check tests" python3 -I "$root/tools/test_check_decision_labels.py"
    step "decision record check tests" python3 -I "$root/tools/test_adr.py"
    step "no-metapac probes" bash "$root/checks/no-metapac/run.sh"
    step "evidence recorder tests" python3 -I "$root/tools/test_record_evidence.py"
    step "rig telemetry tests" python3 -I "$root/tools/test_rig_telemetry.py"
    step "rig entry point tests" python3 -I "$root/tools/test_rig.py"
    # The requirements against the code (a renamed test a requirement names fails here):
    # reqtool from $REQTOOL, or the sibling checkout CI pins (tools/record_evidence.py too).
    # CI has no sibling checkout here: its Requirements job installs reqtool and runs this.
    reqtool="${REQTOOL:-$root/../reqtool/target/release/reqtool}"
    if [ -x "$reqtool" ]; then
        step "requirements valid (reqtool validate)" bash -c 'cd "$1" && "$2" validate' _ "$root" "$reqtool"
    else
        echo "skip  requirements valid: no reqtool at $reqtool (set REQTOOL)"
    fi
    # The examples, each its own workspace: from its directory, so its
    # .cargo/config.toml picks the target and linker scripts.
    for example in "$root"/examples/*/; do
        name=$(basename "$example")
        step "example clippy ($name)" \
            bash -c 'cd "$1" && cargo clippy --release --locked --bins -- -D warnings' _ "$example"
        step "example builds ($name)" bash -c 'cd "$1" && cargo build --release --locked --bins' _ "$example"
        step "example formatting ($name)" bash -c 'cd "$1" && cargo fmt --check' _ "$example"
    done
    step "docs index up to date" python3 -I "$root/tools/docs_index.py" --check
    step "doc size limits" python3 -I "$root/tools/check_docs.py"
    exit "$status"
) 2> "$work/shared.err" || status=1
for job in "${jobs[@]}"; do
    wait "$job"
done
for chip in "${chips[@]}"; do
    cat "$work/$chip.lint"
done
cat "$work/packages.lint"
cat "$work/packages.err" >&2
[ -e "$work/packages.fail" ] && status=1
printf '%s\n' "$shared"
for chip in "${chips[@]}"; do
    cat "$work/$chip.host"
    cat "$work/$chip.err" >&2
    [ -e "$work/$chip.fail" ] && status=1
done
cat "$work/shared.err" >&2
exit "$status"
