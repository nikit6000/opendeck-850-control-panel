#!/usr/bin/env python3
"""Sweep every not-yet-understood byte of MAIN frames 01 / 02 / 80, one after another.

Each candidate gets a number, shown on the display as TRACK. The byte runs 0..255 (step 4) in ~5 s,
then the base value is restored for 1 s. Watch the panel and note the number when something moves.

usage: sweepall.py [START_INDEX]
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')
F80 = bytes.fromhex('80 01 01 00 00 00 00 00 00 02 00 00 02 00 00 00')
F81 = bytes.fromhex('81 11 00 00 00 00 12 00 00 00 00 13 00 00 00 00')
F82 = bytes.fromhex('82 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00')

CANDIDATES = [(1, b) for b in (1, 2, 12, 13, 14)] + [(2, b) for b in (1, 2, 8, 9, 11, 12, 13, 14)] + \
             [(0x80, b) for b in range(3, 14)]


def main():
    start = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
    s = serial.Serial(port, timeout=0.02)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))

    def send(idx, frame=None, byte=None, val=None):
        a, b, c = bytearray(F01), bytearray(F02), bytearray(F80)
        if frame is not None:
            {1: a, 2: b, 0x80: c}[frame][byte] = val
        a[4] = idx  # TRACK = candidate number
        s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(a) + bytes(b))
        s.write(bytes([0xA5, 0x5A, 0x02, 48]) + bytes(c) + F81 + F82)
        s.read(8192)

    try:
        while True:
            for i, (frame, byte) in enumerate(CANDIDATES, 1):
                if i < start:
                    continue
                print(f'#{i:2d}: frame {frame:02x} byte {byte:2d}', flush=True)
                for v in range(0, 256, 4):
                    send(i, frame, byte, v)
                    time.sleep(0.08)
                end = time.time() + 1.0
                while time.time() < end:
                    send(i)
                    time.sleep(0.05)
            start = 1
            print('--- again ---', flush=True)
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
