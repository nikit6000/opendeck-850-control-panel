#!/usr/bin/env python3
"""Sweep one byte of a MAIN frame through the adapter (SLAVE mode) to find what it drives on the panel.

usage: sweep.py FRAME BYTE [FROM TO STEP DELAY]     e.g. sweep.py 2 4 0 135 1 0.3
       sweep.py FRAME BYTE VALUE                    hold one value
       sweep.py FRAME BYTE V1,V2,... DELAY          step through a list

Base frames = native MAIN with a CD track paused at the cue point (captures/jog_cue).
The value being sent is shown on the display: TRACK = value % 100, minutes = value // 100.
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')


def send(s, f01, f02):
    s.write(bytes([0xA5, 0x5A, 0x01, 32]) + f01 + f02)


def main():
    a = sys.argv[1:]
    if len(a) < 3:
        sys.exit(__doc__)
    frame, byte = int(a[0]), int(a[1])
    if ',' in a[2]:
        vals, delay = [int(x, 0) for x in a[2].split(',')], float(a[3]) if len(a) > 3 else 5.0
    elif len(a) == 3:
        vals, delay = [int(a[2], 0)], 1.0
    else:
        lo, hi = int(a[2], 0), int(a[3], 0)
        step = int(a[4], 0) if len(a) > 4 else 1
        delay = float(a[5]) if len(a) > 5 else 0.3
        vals = list(range(lo, hi + 1, step))
    port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
    s = serial.Serial(port, timeout=0.05)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))  # SLAVE (no-op if already)
    try:
        while True:
            for v in vals:
                f01, f02 = bytearray(F01), bytearray(F02)
                (f01 if frame == 1 else f02)[byte] = v
                if not (frame == 1 and byte in (4, 5)):
                    f01[4] = v % 100
                    f01[5] = v // 100
                send(s, bytes(f01), bytes(f02))
                print('\rframe %02x byte %d = %3d (0x%02x)   ' % (frame, byte, v, v), end='', flush=True)
                t_end = time.time() + delay
                while time.time() < t_end:
                    s.read(4096)  # drain records
                    time.sleep(0.01)
            if len(vals) == 1:
                continue
            print()
    except KeyboardInterrupt:
        print()


if __name__ == '__main__':
    main()
