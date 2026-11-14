#!/usr/bin/env bash
# The generated glue's merge gate (ADR-008, plan step 6): regenerates every part's
# routing module (src/chips/gen/) and docs/CHIPS.md's generated tables from the pinned
# stm32-metapac, runs the generator's tests, and fails on any difference from what is
# checked in. CI runs it on pull requests into main; run it on demand anywhere.
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/tools/glue-gen"
cargo test --release --locked --no-default-features --quiet
cargo run --release --locked --no-default-features --quiet -- emit all
cargo run --release --locked --no-default-features --quiet -- tables all
cd "$root"
changed=$(git status --porcelain -- src/chips/gen docs/CHIPS.md)
if [ -n "$changed" ]; then
    echo "FAIL generated glue differs from a regeneration:"
    echo "$changed"
    git --no-pager diff --stat -- src/chips/gen docs/CHIPS.md
    exit 1
fi
echo "pass  generated glue and tables match a regeneration from stm32-metapac"
