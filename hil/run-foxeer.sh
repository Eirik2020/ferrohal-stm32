#!/usr/bin/env bash
# The Foxeer F405 V2 bench. Before its first run, leave FerroWasp with
# `ferro-configurator device reboot --dfu`, or hold BOOT while plugging in.
# Its stop record: the top 32 bytes of SRAM (src/ident.rs, STOP_RECORD).
STOP_RECORD=0x2001FFE0 exec "$(cd "$(dirname "$0")" && pwd)/flash-dfu.sh" foxeer-f405-v2 "04*016Kg"
