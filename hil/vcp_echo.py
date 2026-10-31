"""Send every byte that arrives on a serial port straight back.

The F401 HIL run A loops USART2 through the ST-LINK's virtual COM port: the
board transmits, this script returns the bytes, the board checks them.

    python3 -I vcp_echo.py /dev/serial/by-id/usb-...-if02 [baud]

Runs until killed. Raw mode: no line discipline, no echo by the tty itself.
"""

import os
import select
import signal
import sys
import termios

port = sys.argv[1]
baud = int(sys.argv[2]) if len(sys.argv) > 2 else 115_200
speed = getattr(termios, f"B{baud}")

fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
attrs = termios.tcgetattr(fd)
attrs[0] = 0  # iflag
attrs[1] = 0  # oflag
attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL  # cflag
attrs[3] = 0  # lflag: raw
attrs[4] = attrs[5] = speed
attrs[6][termios.VMIN] = 0
attrs[6][termios.VTIME] = 0
termios.tcsetattr(fd, termios.TCSANOW, attrs)
termios.tcflush(fd, termios.TCIOFLUSH)

signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
print(f"echoing {port} at {baud} baud", flush=True)

echoed = 0
try:
    while True:
        ready, _, _ = select.select([fd], [], [], 1.0)
        if not ready:
            continue
        data = os.read(fd, 4096)
        while data:
            # Non-blocking: write what the port takes, wait, write the rest.
            try:
                written = os.write(fd, data)
            except BlockingIOError:
                select.select([], [fd], [], 1.0)
                continue
            data = data[written:]
            echoed += written
finally:
    os.close(fd)
    print(f"echoed {echoed} bytes", flush=True)
