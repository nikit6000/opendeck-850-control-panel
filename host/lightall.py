#!/usr/bin/env python3
"""Light everything on the panel: all bits of MAIN frame 02 bytes 5..14 (except STANDBY, byte 8 bit 0x01)
and frame 01 byte 2 set,
ring + jog cue mark at fixed positions, 88:88 on the display. Holds until Ctrl-C.

usage: lightall.py [RING] [MARK]
"""
import glob, sys, time

import serial

ring = int(sys.argv[1]) if len(sys.argv) > 1 else 40
mark = int(sys.argv[2]) if len(sys.argv) > 2 else 60
f01 = bytearray.fromhex('01 3f ff 64 58 58 3b 4a 03 e7 27 0f 00 00 00 00')
f02 = bytearray.fromhex('02 3f ff 64 64 ff ff ff ff ff ff ff ff ff ff 00')
f01[1] = f02[1] = mark
f01[3] = f02[3] = ring
f02[8] &= ~0x01  # byte 8 bit 0x01 = STANDBY (switches the player off) — never set it here
f02[4] = 50  # position bar half way
f02[10] = 75  # cue dot

port = sorted(glob.glob('/dev/cu.usbmodem*'))[0]
s = serial.Serial(port, timeout=0.05)
s.dtr = True
s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))
print('all on — Ctrl-C to stop')
try:
    while True:
        s.write(bytes([0xA5, 0x5A, 0x01, 32]) + bytes(f01) + bytes(f02))
        s.read(8192)
        time.sleep(0.2)
except KeyboardInterrupt:
    pass
