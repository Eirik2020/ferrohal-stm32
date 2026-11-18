#!/usr/bin/env bash
# The generated glue's merge gate (ADR-008, ADR-009, ADR-012): regenerates every part's
# routing module (src/chips/gen/), the chip selection in src/lib.rs, and docs/CHIPS.md's
# tables from the pinned stm32-metapac, and
# the tier tables in docs/CHIPS.md and hil/COVERAGE.md from reqtool's evidence, runs the
# generator's tests, and fails on any difference from what is checked in. CI runs it on
# pull requests into main; run it on demand anywhere, with reqtool on the PATH or REQTOOL.
#
# Each part's metapac dump is cached (tools/glue-gen/target/dump-cache, or GLUE_DUMP_CACHE;
# `off` disables it) under a key covering all it depends on; every part is still regenerated.
#
# `--no-tiers` leaves the tier tables alone: the glue and the silicon tables only. The
# evidence step (tools/record_evidence.py prepare) runs it that way, because the tiers
# follow from the very evidence it is about to record, so they differ until then.
set -eu
tiers=1
[ "${1:-}" = --no-tiers ] && tiers=0
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/tools/glue-gen"
cargo test --release --locked --no-default-features --quiet
cargo run --release --locked --no-default-features --quiet -- emit all
cargo run --release --locked --no-default-features --quiet -- tables all
# The tiers, from reqtool's evidence (REQTOOL, or reqtool on the PATH).
[ "$tiers" -eq 1 ] && cargo run --release --locked --no-default-features --quiet -- tiers all
cd "$root"
changed=$(git status --porcelain -- src/chips/gen src/lib.rs docs/CHIPS.md hil/COVERAGE.md)
if [ -n "$changed" ]; then
    echo "FAIL generated glue differs from a regeneration:"
    echo "$changed"
    git --no-pager diff --stat -- src/chips/gen src/lib.rs docs/CHIPS.md hil/COVERAGE.md
    exit 1
fi
if [ "$tiers" -eq 1 ]; then
    echo "pass  generated glue and tables match a regeneration from stm32-metapac and the evidence"
else
    echo "pass  generated glue and silicon tables match a regeneration from stm32-metapac"
fi
