#!/usr/bin/env bash
# The Foxeer F405 V2 bench. Before its first run, leave FerroWasp with
# `ferro-configurator device reboot --dfu`, or hold BOOT while plugging in.
exec "$(cd "$(dirname "$0")" && pwd)/run-dfu.sh" foxeer-f405-v2 "04*016Kg"
