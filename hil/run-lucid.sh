#!/usr/bin/env bash
# The TBS Lucid H7 bench. Before its first run, hold BOOT while plugging in.
exec "$(cd "$(dirname "$0")" && pwd)/flash-dfu.sh" lucid-h7 "16*128Kg"
