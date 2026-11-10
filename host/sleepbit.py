#!/usr/bin/env python3
"""Find the MAIN-frame bit that puts the player to sleep.

Holds one candidate bit at a time for HOLD seconds (number shown as TRACK) on top of quiet frames.
The panel clocks the bus only while it is on, so "no frames for 1 s" = the player switched off:
the candidate is reported, the script waits until frames come back (turn the player on), then goes on.

usage: sleepbit.py [START_INDEX] [HOLD]
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')

CANDIDATES = [(2, 8, 1 << k) for k in range(8)] + \
             [(1, 2, m) for m in (0x01, 0x02, 0x04, 0x80)] + \
             [(2, 9, 0x01), (2, 9, 0x02)] + \
             [(1, 12, 1 << k) for k in range(8)] + \
             [(1, 13, 1 << k) for k in range(8)] + \
             [(1, 14, 1 << k) for k in range(8)] + \
             [(2, 13, 0x10), (2, 13, 0x40), (2, 13, 0x80), (2, 12, 0x02), (2, 12, 0x04),
              (2, 14, 0x01), (2, 14, 0x10), (2, 11, 0x20), (2, 11, 0x40), (2, 5, 0x01),
              (2, 6, 0x04), (2, 6, 0x80)]


def main():
    start = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    hold = float(sys.argv[2]) if len(sys.argv) > 2 else 4.0
    port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
    s = serial.Serial(port, timeout=0.05)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))
    found = []

    def send(idx, cand=None):
        a, b = bytearray(F01), bytearray(F02)
        if cand:
            fr, by, m = cand
            (a if fr == 1 else b)[by] ^= m
        a[4] = idx
        s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(a) + bytes(b))

    def frames(sec, idx, cand=None):
        """send for `sec` seconds; return False if the bus went silent for 1 s"""
        end = time.time() + sec
        last = time.time()
        while time.time() < end:
            send(idx, cand)
            if s.read(4096):
                last = time.time()
            elif time.time() - last > 1.0:
                return False
            time.sleep(0.05)
        return True

    def wait_on(idx):
        print('   … waiting for the player (turn it on)', flush=True)
        while True:
            send(idx)
            if s.read(4096):
                frames(4.0, idx)  # handshake + settle
                return
            time.sleep(0.1)

    try:
        if not frames(1.5, 0):
            wait_on(0)
        for i, c in enumerate(CANDIDATES, 1):
            if i < start:
                continue
            print(f'#{i:2d}: frame {c[0]:02x} byte {c[1]:2d} bit {c[2]:#04x}', flush=True)
            if not frames(hold, i, c):
                print(f'   *** player went to sleep on #{i}: frame {c[0]:02x} byte {c[1]} bit {c[2]:#04x}', flush=True)
                found.append(c)
                wait_on(i)
            frames(1.0, i)
        print('done. sleep bits:', [f'frame {f:02x} byte {b} bit {m:#04x}' for f, b, m in found] or 'none', flush=True)
    except KeyboardInterrupt:
        print('stopped. sleep bits so far:', found)


if __name__ == '__main__':
    main()
