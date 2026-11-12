#!/usr/bin/env bash
# Compile-fail probes for G5 (docs/policies/safety.md) and the free() ownership
# rules (docs/design/free.md): every bin in src/bin/ names a register hatch or
# an ownership misuse that must stay closed. A probe must FAIL to compile with
# the error code its doc names (`Expects: EXXXX`): a probe that compiles means
# a hatch is open again, and a probe that fails with another error means the
# probe itself rotted. Exits non-zero naming the probe.
set -u
here="$(cd "$(dirname "$0")" && pwd)"
status=0
count=0
for probe in "$here"/src/bin/*.rs; do
    name=$(basename "$probe" .rs)
    count=$((count + 1))
    expected=$(sed -n 's/^\/\/[!/] Expects: \(E[0-9]*\)$/\1/p' "$probe")
    if [ -z "$expected" ]; then
        echo "FAIL no-metapac $name: no 'Expects: EXXXX' doc line"
        status=1
        continue
    fi
    if out=$(cd "$here" && cargo build --bin "$name" 2>&1); then
        echo "FAIL no-metapac $name: compiled; the hatch is open"
        status=1
    elif ! printf '%s\n' "$out" | grep -q "error\[$expected\]"; then
        echo "FAIL no-metapac $name: failed without error[$expected]"
        printf '%s\n' "$out" | grep '^error' | head -3
        status=1
    fi
done
if [ "$status" -eq 0 ]; then
    echo "pass  no-metapac probes ($count hatches closed)"
fi
exit "$status"
