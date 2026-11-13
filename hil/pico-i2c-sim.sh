#!/usr/bin/env bash
# Flashes the Pico 1 I2C simulator (BOOTSEL/UF2 — the Pico has no
# on-board debug probe; its USB cable is the one taken from the
# Nucleo-H753ZI's CN13, see hil/WIRING.md), then runs the two bench runs
# that speak to it: F401 run S (I2C3 on PA8/PC9) and H753 run S (I2C4 on
# PD12/PD13). One combined log lands in hil/logs/i2c-sim.log. Exits
# non-zero if the build or either run failed.
#
# `pico-i2c-sim.sh flash` builds the firmware and writes the UF2 next to
# it (and onto the Pico's RPI-RP2 drive, if it is mounted); the Pico
# keeps its state across runs, so one flash serves any number of them.
# `pico-i2c-sim.sh` without arguments assumes the Pico already runs the
# firmware and just runs the benches.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$here/logs"
firmware="$here/pico-i2c-sim/target/thumbv6m-none-eabi/release/pico-i2c-sim"
summary="$here/logs/i2c-sim.log"

if [ "${1:-}" = flash ]; then
    echo "== building the Pico firmware"
    (cd "$here/pico-i2c-sim" && cargo build --release) || exit 1
    if command -v elf2uf2-rs > /dev/null; then
        echo "== writing $firmware.uf2"
        elf2uf2-rs "$firmware" "$firmware.uf2" || exit 1
        drive=""
        for candidate in /media/*/RPI-RP2 /run/media/*/RPI-RP2; do
            [ -d "$candidate" ] && drive="$candidate"
        done
        if [ -z "$drive" ]; then
            # No finger needed: the running simulator reboots itself into
            # its bootloader when the F401's run T asks it to (the map's
            # BOOT register). Only a Pico with no simulator — or one whose
            # firmware predates run T — still wants BOOTSEL by hand.
            echo "== no RPI-RP2 drive: asking the simulator for its bootloader (run T)"
            "$here/run-f401.sh" t > /dev/null 2>&1 || true
            for _ in $(seq 1 40); do
                for candidate in /media/*/RPI-RP2 /run/media/*/RPI-RP2; do
                    [ -d "$candidate" ] && drive="$candidate"
                done
                [ -n "$drive" ] && break
                sleep 0.5
            done
        fi
        if [ -z "$drive" ]; then
            echo "== the drive never appeared; hold BOOTSEL while plugging the"
            echo "   Pico's USB cable, then run 'pico-i2c-sim.sh flash' again"
            exit 1
        fi
        echo "== copying onto $drive"
        cp "$firmware.uf2" "$drive/" || exit 1
    else
        echo "== elf2uf2-rs not installed; hold BOOTSEL while plugging the"
        echo "   Pico's USB cable and drop the UF2 onto its drive, or:"
        echo "   cargo install elf2uf2-rs --locked"
        echo "   elf2uf2-rs $firmware $firmware.uf2"
    fi
    exit 0
fi

status=0
{
    echo "FerroHAL I2C simulator bench $(date -Is)"

    echo "== F401 run S (I2C3, Pico bus tag 1)"
    "$here/run-f401.sh" s || status=1

    echo "== H753 run S (I2C4, Pico bus tag 2)"
    "$here/run-h753.sh" s || status=1
} > "$summary" 2>&1
cat "$summary"
[ "$status" -eq 0 ] && echo "both simulator runs passed" || echo "simulator bench failed"
exit "$status"
