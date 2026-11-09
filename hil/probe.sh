#!/usr/bin/env bash
# Picks the debug probe for a bench, by the selector `probe-rs list` prints
# after `--` (VID:PID:Serial), and exports it as PROBE_RS_PROBE, which
# probe-rs reads for `--probe`.
#
# Order: PROBE_RS_PROBE if already set, then the board's own variable, then
# the only probe attached. With several attached and none chosen it fails and
# lists them, so a shared bench says which board it is flashing.
#
#   source hil/probe.sh
#   probe_for F401_PROBE || exit 1

probe_for() {
    local board_var=$1
    if [ -n "${PROBE_RS_PROBE:-}" ]; then
        export PROBE_RS_PROBE
        return 0
    fi
    local chosen="${!board_var:-}"
    if [ -z "$chosen" ]; then
        local probes count
        probes=$(probe-rs list 2>/dev/null | sed -n 's/.* -- \([^ ]*\) (.*/\1/p')
        count=$(printf '%s\n' "$probes" | grep -c . || true)
        if [ "$count" -ne 1 ]; then
            echo "probe: set $board_var (or PROBE_RS_PROBE) to one of these," >&2
            echo "probe: as probe-rs list prints it after '--':" >&2
            printf '  %s\n' "$probes" >&2
            return 1
        fi
        chosen=$probes
    fi
    export PROBE_RS_PROBE="$chosen"
}
