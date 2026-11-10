#!/usr/bin/env python3
"""Blink one bit of a MAIN frame at a time (1 Hz) to see what it drives on the panel.

The candidate number is shown on the display as TRACK. Base frames = native MAIN, track paused at the
cue point (captures/jog_cue). Each candidate blinks for SECONDS (default 7), then the next one.

usage: bits.py [START_INDEX] [SECONDS]
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')

# (frame, byte, mask, note)
CANDIDATES = [
    (2, 5, 0x02, 'USB access?'),
    (2, 6, 0x01, 'RELOOP?'),
    (2, 6, 0x40, 'disc slot?'),
    (2, 7, 0x80, 'TAG TRACK?'),
    (2, 9, 0x01, 'track end 1?'),
    (2, 9, 0x02, 'track end 2?'),
    (2, 9, 0x04, 'track end 3?'),
    (2, 11, 0x04, '?'),
    (2, 11, 0x08, '?'),
    (2, 11, 0x10, '?'),
    (2, 12, 0x08, 'cue mark on?'),
    (2, 12, 0x80, '?'),
    (2, 13, 0x10, '?'),
    (2, 13, 0x40, '?'),
    (2, 13, 0x80, '?'),
    (2, 12, 0x02, '?'),
    (2, 12, 0x04, '?'),
    (2, 14, 0x01, '?'),
    (2, 14, 0x10, '?'),
    (2, 11, 0x20, '?'),
    (2, 11, 0x40, '?'),
    (2, 5, 0x01, '?'),
    (2, 6, 0x04, '?'),
    (2, 6, 0x80, '?'),
    (1, 2, 0x01, '?'),
    (1, 2, 0x02, '?'),
    (1, 2, 0x04, '?'),
    (1, 2, 0x80, '?'),
]


def main():
    start = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    secs = float(sys.argv[2]) if len(sys.argv) > 2 else 7.0
    port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
    s = serial.Serial(port, timeout=0.02)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))

    def send(idx, cand=None, on=False):
        a, b = bytearray(F01), bytearray(F02)
        if cand and on:
            fr, by, m, _ = cand
            (a if fr == 1 else b)[by] ^= m
        a[4] = idx
        s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(a) + bytes(b))
        s.read(8192)

    try:
        while True:
            for i, c in enumerate(CANDIDATES, 1):
                if i < start:
                    continue
                print(f'#{i:2d}: frame {c[0]:02x} byte {c[1]:2d} bit {c[2]:#04x}  {c[3]}', flush=True)
                end = time.time() + secs
                while time.time() < end:
                    on = int((end - time.time()) * 2) % 2 == 0
                    send(i, c, on)
                    time.sleep(0.05)
            start = 1
            print('--- again ---', flush=True)
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
