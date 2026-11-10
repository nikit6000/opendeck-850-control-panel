#!/usr/bin/env python3
"""Narrow down the 'player goes dark' bit: hold groups of candidate bits, 8 s each, 6 s quiet between.

Groups (from sleepbit.py candidates #2..#10):
  A: frame 02 byte 8 = 0x1E (bits 0x02 0x04 0x08 0x10)
  B: frame 02 byte 8 = 0xE0 (bits 0x20 0x40 0x80)
  C: frame 01 byte 2 ^ 0x03 (bits 0x01 0x02)
The display shows the group as TRACK (1 = A, 2 = B, 3 = C) while it is held; 0 during the quiet gap.

usage: sleepgroups.py [GROUPS]   e.g. sleepgroups.py ABC  or  sleepgroups.py A
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')
GROUPS = {'A': [(2, 8, 0x1E)], 'B': [(2, 8, 0xE0)], 'C': [(1, 2, 0x03)],
          # single bits for the final step
          **{f'8.{m:02x}': [(2, 8, m)] for m in (0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80)},
          '2.01': [(1, 2, 0x01)], '2.02': [(1, 2, 0x02)], '8.01': [(2, 8, 0x01)]}


def main():
    names = sys.argv[1:] or ['A', 'B', 'C']
    if len(names) == 1 and set(names[0]) <= set('ABC') and len(names[0]) > 1:
        names = list(names[0])
    s = serial.Serial(sorted(glob.glob('/dev/cu.usbmodem*'))[0], timeout=0.05)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))

    def hold(sec, idx, bits):
        end = time.time() + sec
        while time.time() < end:
            a, b = bytearray(F01), bytearray(F02)
            for fr, by, m in bits:
                (a if fr == 1 else b)[by] ^= m
            a[4] = idx
            s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(a) + bytes(b))
            s.read(8192)
            time.sleep(0.05)

    try:
        for n, name in enumerate(names, 1):
            print(f'{n}. group {name} ON  {GROUPS[name]}', flush=True)
            hold(8, n, GROUPS[name])
            print(f'   group {name} off', flush=True)
            hold(6, 0, [])
        print('done', flush=True)
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
