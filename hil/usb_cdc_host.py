"""The host side of the USB CDC bench: finds the board's serial port, sends
blocks of every interesting size, and checks each byte that comes back.

    python3 -I usb_cdc_host.py [seconds to wait for the port] [board serial] [report]

A board serial picks that board's port when several are connected. With
`report` (a board with no probe, reporting over this port), asks the board to
run its checks and prints its report first, then asks it to echo.

`--fresh` puts the wait before the flash: a port a previous image left has to go
before the host opens anything, so a rerun cannot talk to the image it is about
to replace. Without it the host takes the first port that matches, which is the
flashed image's own when the caller flashed first and waited for the old port to
go (run-dfu.sh). `FERROHAL_SERIAL_GLOB` overrides the by-id pattern (a bench
whose udev names differ, or a self-test with a fake directory).

The sizes around 64 bytes straddle the full-speed packet size, where a
transfer ending on a whole packet needs a zero-length packet to finish. Ends
the board's run with the END marker. Exits non-zero on any mismatch.
"""

import glob
import os
import select
import sys
import termios
import time

END = b"BENCHEND"
RUN = b"BENCHRUN"
ECHO = b"BENCHECH"
args = [argument for argument in sys.argv[1:] if argument != "--fresh"]
fresh = len(args) != len(sys.argv) - 1
wait = float(args[0]) if args else 30.0
board = args[1] if len(args) > 1 else ""
reports = len(args) > 2


def signature(path):
    """What tells a port from the one a previous image left, even if udev reuses its name."""
    stat = os.stat(path)
    return (os.path.realpath(path), stat.st_ino, stat.st_rdev)


def find_port():
    pattern = os.environ.get("FERROHAL_SERIAL_GLOB") or f"/dev/serial/by-id/*FerroHAL*{board}*"
    deadline = time.monotonic() + wait
    stale = sorted(glob.glob(pattern)) if fresh else []
    stale_signature = signature(stale[0]) if stale else None
    gone = not stale
    while time.monotonic() < deadline:
        ports = sorted(glob.glob(pattern))
        if ports:
            try:
                found = signature(ports[0])
            except FileNotFoundError:
                found = None
            if found is not None and (not fresh or gone or found != stale_signature):
                # udev has just made it; give permissions a moment to settle.
                time.sleep(0.3)
                return ports[0]
        else:
            gone = True
        time.sleep(0.02)
    sys.exit(
        f"FAIL no {'fresh ' if fresh else ''}FerroHAL serial port within {wait:.0f} s: is a data cable "
        "in the user USB port? A charge-only cable gives the board VBUS and the host nothing."
        + (
            " A port the previous image left has to go before its replacement appears; a cable "
            "that stays leaves it there."
            if fresh
            else ""
        )
    )


port = find_port()
fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
attrs = termios.tcgetattr(fd)
attrs[0] = 0
attrs[1] = 0
attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
attrs[3] = 0
attrs[6][termios.VMIN] = 0
attrs[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, attrs)
termios.tcflush(fd, termios.TCIOFLUSH)
print(f"PASS host: port {os.path.basename(port)}", flush=True)


def pattern(size, seed):
    return bytes(((i * 2654435761 + seed * 40503) >> 24) & 0xFF for i in range(size))


def echo(data, timeout=10.0):
    """Sends `data`, returns what came back, stopping at its length."""
    sent, got = 0, bytearray()
    deadline = time.monotonic() + timeout
    while len(got) < len(data) and time.monotonic() < deadline:
        want_write = [fd] if sent < len(data) else []
        readable, writable, _ = select.select([fd], want_write, [], 0.2)
        if writable:
            try:
                sent += os.write(fd, data[sent : sent + 512])
            except BlockingIOError:
                pass
        if readable:
            got += os.read(fd, 4096)
    return bytes(got)


def report(timeout=60.0):
    """Runs the board's checks, printing its report; returns how many failed."""
    os.write(fd, RUN)
    text = bytearray()
    deadline = time.monotonic() + timeout
    failures = 0
    while time.monotonic() < deadline:
        if select.select([fd], [], [], 0.2)[0]:
            text += os.read(fd, 4096)
        while b"\n" in text:
            line, _, rest = text.partition(b"\n")
            text = bytearray(rest)
            line = line.decode(errors="replace")
            print(line, flush=True)
            failures += line.startswith("FAIL")
            if line.startswith("DONE"):
                return failures
    print(f"FAIL host: no DONE within {timeout:.0f} s", flush=True)
    return failures + 1


failed = 0
if reports:
    failed += report()
    os.write(fd, ECHO)
for seed, size in enumerate([1, 63, 64, 65, 128, 512, 4096]):
    data = pattern(size, seed)
    back = echo(data)
    if back == data:
        print(f"PASS host: {size} bytes echoed", flush=True)
    else:
        failed += 1
        at = next((i for i, (a, b) in enumerate(zip(data, back)) if a != b), min(len(data), len(back)))
        print(f"FAIL host: {size} bytes: {len(back)} came back, first difference at {at}", flush=True)

data = pattern(64 * 1024, 99)
start = time.monotonic()
back = echo(data, 30.0)
elapsed = time.monotonic() - start
if back == data:
    print(f"PASS host: 64 KiB echoed at {len(data) / elapsed / 1024:.0f} KiB/s", flush=True)
else:
    failed += 1
    print(f"FAIL host: 64 KiB: {len(back)} bytes came back", flush=True)

os.write(fd, END)
time.sleep(0.2)
os.close(fd)
sys.exit(1 if failed else 0)
