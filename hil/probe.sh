#!/usr/bin/env bash
# Picks the debug probe for a bench, by the selector `probe-rs list` prints
# after `--` (VID:PID:Serial), and exports it as PROBE_RS_PROBE, which
# probe-rs reads for `--probe`.
#
# Order: PROBE_RS_PROBE if already set, then the board's own variable, then
# the only probe attached, then the one probe whose die has the bench's
# DEV_ID (read over each probe without halting, as hil/rig_status.py does).
# With several matching, or none, it fails and lists the probes.
#
#   source hil/probe.sh
#   probe_for F401_PROBE 0x433 || exit 1

# The DEV_IDs read behind probe $1, one per line: DBGMCU_IDCODE's low 12 bits
# at 0xE0042000 (F4, F7, G4) and at 0x5C001000 (H7). Only the die's own
# register holds its DEV_ID, so a caller matches on any line.
dev_ids() {
    local spec word
    for spec in "Cortex-M4 0xE0042000" "Cortex-M7 0x5C001000"; do
        set -- "$1" $spec
        # Twice: a probe another process holds for a moment refuses to open.
        word=$(timeout 20 probe-rs read --chip "$2" --probe "$1" b32 "$3" 1 2> /dev/null \
            | sed -n 's/^[0-9a-f]*: \([0-9a-f]*\).*/\1/p')
        [ -z "$word" ] && sleep 1 && word=$(timeout 20 probe-rs read --chip "$2" --probe "$1" b32 "$3" 1 2> /dev/null \
            | sed -n 's/^[0-9a-f]*: \([0-9a-f]*\).*/\1/p')
        [ -n "$word" ] && printf '0x%03x\n' $((0x$word & 0xfff))
    done
}

probe_for() {
    local board_var=$1 dev=${2:-}
    if [ -n "${PROBE_RS_PROBE:-}" ]; then
        export PROBE_RS_PROBE
        return 0
    fi
    local chosen="${!board_var:-}"
    if [ -z "$chosen" ]; then
        local probes count
        probes=$(probe-rs list 2>/dev/null | sed -n 's/.* -- \([^ ]*\) (.*/\1/p')
        count=$(printf '%s\n' "$probes" | grep -c . || true)
        if [ "$count" -ne 1 ] && [ -n "$dev" ]; then
            local probe matches="" lock
            # Bench scripts started together would read every probe at once,
            # and a probe opens for one process only: they take turns.
            exec {lock}> "${TMPDIR:-/tmp}/ferrohal-probe-select.lock"
            flock "$lock"
            for probe in $probes; do
                dev_ids "$probe" | grep -qx "$(printf '0x%03x' $((dev)))" && matches="$matches $probe"
            done
            flock -u "$lock"
            exec {lock}>&-
            if [ "$(printf '%s\n' $matches | grep -c .)" -eq 1 ]; then
                probes=${matches# }
                count=1
                echo "probe: $board_var unset; $probes has DEV_ID $dev" >&2
            fi
        fi
        if [ "$count" -ne 1 ]; then
            echo "probe: set $board_var (or PROBE_RS_PROBE) to one of these," >&2
            echo "probe: as probe-rs list prints it after '--':" >&2
            printf '%s\n' "$probes" | sed 's/^/  /' >&2
            return 1
        fi
        chosen=$probes
    fi
    export PROBE_RS_PROBE="$chosen"
}
