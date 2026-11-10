#!/usr/bin/env python3
"""Show text on the panel's dot-matrix display through the adapter (MAIN frames 80 + 81..8f).

Frame 80 = screen: byte 1 sequence number (+1 per screen change, the panel acks it), byte 2 screen id
(0x01 track, 0x07 popup, 0x08 no media). Frames 81, 82, … carry a record stream in their bytes 1..14:
[id][3 attr bytes][len][len chars]. Native track screen: 0x25 = file name / title line (attr 01 00 00),
0x02 = 2nd line, 0x03 = 3rd line. No-media screen: 0x11 / 0x12 (message, e.g. "NO DISC") / 0x13.

usage: text.py [SCREEN] "line 25" ["line 02"] ["line 03"]      e.g. text.py 1 "HELLO FROM PI" "ARTIST" "TITLE"
       text.py raw ID:ATTRHEX:TEXT ...                          e.g. text.py raw 12:000000:HELLO
Holds until Ctrl-C.
"""
import glob, sys, time

import serial

F01 = bytes.fromhex('01 02 58 03 01 04 32 4a 00 ae 00 00 00 00 00 00')  # led_base: track loaded, at cue
F02 = bytes.fromhex('02 02 58 03 01 78 62 1f 00 04 01 04 59 2f 4a 00')
F80_TAIL = bytes.fromhex('00 01 00 10 00 00 02 00 20 03 00 00')       # led_base frame 80 bytes 3..14


def records_to_frames(recs):
    stream = b''.join(bytes([rid]) + attr + bytes([len(t)]) + t for rid, attr, t in recs)
    frames = []
    for i in range(0, max(len(stream), 1), 14):
        chunk = stream[i:i + 14].ljust(14, b'\0')
        frames.append(bytes([0x81 + len(frames)]) + chunk + b'\0')
    return frames


def main():
    a = sys.argv[1:]
    if not a:
        sys.exit(__doc__)
    if a[0] == 'raw':
        screen = 0x01
        recs = []
        for spec in a[1:]:
            rid, attr, t = spec.split(':', 2)
            recs.append((int(rid, 16), bytes.fromhex(attr), t.encode('latin1')))
    else:
        screen = int(a[0], 0) if a[0].isdigit() or a[0].startswith('0x') else 1
        lines = a[1:] if screen != 1 or a[0].isdigit() or a[0].startswith('0x') else a
        ids = [0x25, 0x02, 0x03]
        recs = [(ids[i], b'\x01\x00\x00', t.encode('latin1')) for i, t in enumerate(lines[:3])]
    frames = records_to_frames(recs)
    if len(frames) > 14:
        sys.exit('text too long')
    seq = int(time.time()) & 0xFF
    f80 = bytes([0x80, seq, screen]) + F80_TAIL + b'\0'
    s = serial.Serial(sorted(glob.glob('/dev/cu.usbmodem*'))[0], timeout=0.05)
    s.dtr = True
    s.write(bytes([0xA5, 0x5A, 0x03, 1, 2]))
    others = f80 + b''.join(frames)
    print(f'screen {screen:#04x} seq {seq}: ' + ', '.join(f'{r:#04x} {t!r}' for r, _, t in recs))
    try:
        while True:
            s.write(bytes([0xA5, 0x5A, 0x01, 32]) + F01 + F02)
            s.write(bytes([0xA5, 0x5A, 0x02, len(others)]) + others)
            s.read(8192)
            time.sleep(0.3)
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
