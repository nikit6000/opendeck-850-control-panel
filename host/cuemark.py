#!/usr/bin/env python3
"""Experiment: does the panel latch the jog cue mark at the ring position when a bit turns on?

For ring positions R (frames 01/02 byte 3, 1..135) it holds the ring at R, turns the tested bit
off for 0.5 s and on again, then waits. Watch whether the cue mark jumps to the ring.
TRACK on the display = step number.

usage: cuemark.py [b12|b13|both]
       cuemark.py state FROM TO   — instead of a bit, switch frame 01/02 byte 1 (player state) FROM → TO
       cuemark.py equal           — byte 1 = ring value (is byte 1 the cue-mark angle on the ring scale?)
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 3f 58 64 01 02 3a 07 00 82 00 00 00 00 00 00')
F02 = bytes.fromhex('02 3f 58 64 27 68 62 40 00 04 27 00 d9 2f 4a 00')


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else 'b12'
    st_from = int(sys.argv[2], 0) if which == 'state' else None
    st_to = int(sys.argv[3], 0) if which == 'state' else None
    port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
    s = serial.Serial(port, timeout=0.05)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))

    def send(ring, bit_on, step):
        a, b = bytearray(F01), bytearray(F02)
        a[3] = b[3] = ring
        a[4] = step
        if which == 'equal':
            a[1] = b[1] = ring
        elif which == 'state':
            a[1] = b[1] = st_to if bit_on else st_from
        elif not bit_on:
            if which in ('b12', 'both'):
                b[12] &= ~0x08
            if which in ('b13', 'both'):
                b[13] &= ~0x20
        s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(a) + bytes(b))

    def hold(sec, *args):
        end = time.time() + sec
        while time.time() < end:
            send(*args)
            s.read(4096)
            time.sleep(0.05)

    step = 1
    try:
        while True:
            for ring in (17, 51, 85, 119):  # ~ 2, 5, 8, 11 o'clock
                print(f'step {step}: ring {ring}, mode {which}', flush=True)
                hold(1.0, ring, True, step)
                hold(0.5, ring, False, step)
                hold(3.0, ring, True, step)
                step += 1
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
