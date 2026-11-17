#!/usr/bin/env bash
# The Foxeer F405 V2 bench. Before its first run, leave FerroWasp with
# `ferro-configurator device reboot --dfu`, or hold BOOT while plugging in.
# Its stop record (src/ident.rs, STOP_RECORD) gets its round trip.
STOP_RECORD_PROBE=1 exec "$(cd "$(dirname "$0")" && pwd)/flash-dfu.sh" foxeer-f405-v2 "04*016Kg"
