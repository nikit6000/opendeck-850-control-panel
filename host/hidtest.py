#!/usr/bin/env python3
"""Check the adapter's HID (CDJ-850 PC mode + STM32 extensions) on the bench. Needs `pip install hidapi`.

usage: hidtest.py monitor        presses/releases (incl. byte-5 extensions), tempo, selector, VINYL SPEED, jog
       hidtest.py jog            turn the platter exactly one full turn (pick a mark), then Ctrl-C: counts per turn
       hidtest.py lamps          every lamp on for 2 s, then one by one (name printed), then off
       hidtest.py ring           jog ring sweeps 1..135, cue mark 0..84, position bar and cue dot 0..100 %, time runs
"""
import ctypes, struct, sys, time

import hid

VID, PID = 0x08E4, 0x0157
try:  # macOS hidapi opens exclusively by default
    ctypes.CDLL(hid.__file__).hid_darwin_set_open_exclusive(0)
except Exception:
    pass

BUTTONS = {
    (0, 0x01): "DISC EJECT", (0, 0x04): "TRACK |<<", (0, 0x08): "TRACK >>|", (0, 0x10): "SEARCH <<",
    (0, 0x20): "SEARCH >>", (0, 0x40): "CUE", (0, 0x80): "PLAY",
    (1, 0x20): "RELOOP/EXIT", (1, 0x40): "LOOP OUT", (1, 0x80): "LOOP IN",
    (2, 0x08): "TIME MODE", (2, 0x10): "DELETE", (2, 0x20): "CALL <", (2, 0x40): "CALL >", (2, 0x80): "MEMORY",
    (3, 0x02): "VINYL", (3, 0x08): "MASTER TEMPO", (3, 0x10): "TEMPO RANGE", (3, 0x20): "SELECTOR push",
    (3, 0x40): "BACK",
    (4, 0x08): "BEAT SELECT", (4, 0x10): "AUTO LOOP 1", (4, 0x20): "AUTO LOOP 2", (4, 0x40): "AUTO LOOP 3",
    (4, 0x80): "AUTO LOOP 4",
    (5, 0x01): "BROWSE*", (5, 0x02): "TAG LIST*", (5, 0x04): "INFO*", (5, 0x08): "MENU*", (5, 0x10): "USB STOP*",
    (5, 0x20): "PC*", (5, 0x40): "USB*", (5, 0x80): "DISC*",
    (6, 0x01): "TAG TRACK", (7, 0x01): "DIRECTION REV", (7, 0x20): "JOG TOUCH",
}
LAMPS = [
    (4, 0x80, "PLAY"), (4, 0x40, "CUE"), (4, 0x20, "LOOP IN"), (4, 0x10, "LOOP OUT"), (4, 0x08, "RELOOP/EXIT"),
    (4, 0x02, "REMAIN (display)"), (4, 0x01, "A.CUE (display)"), (5, 0x80, "MASTER TEMPO"), (5, 0x40, "+-6"),
    (5, 0x20, "+-10"), (5, 0x10, "+-16"), (5, 0x08, "WIDE"), (5, 0x01, "MEMORY (display)"),
    (6, 0x01, "AUTO LOOP 1"), (6, 0x02, "AUTO LOOP 2"), (6, 0x04, "AUTO LOOP 3"), (6, 0x08, "AUTO LOOP 4"),
    (6, 0x10, "REV"), (6, 0xC0, "VINYL"), (7, 0x01, "jog touch ring"), (8, 0x01, "BROWSE"), (8, 0x04, "INFO"),
    (8, 0x40, "MENU"), (8, 0x80, "TAG LIST"), (29, 0x01, "USB button*"), (29, 0x02, "LINK*"), (29, 0x04, "DISC*"),
    (29, 0x08, "DISC EJECT*"), (29, 0x10, "USB source*"),
]


def open_dev():
    d = hid.device()
    d.open(VID, PID)
    d.set_nonblocking(True)
    print("opened:", d.get_manufacturer_string(), "/", d.get_product_string())
    return d


def send(d, o):
    d.write(b"\x00" + bytes(o))


def out(ext=True):
    o = bytearray(36)
    o[0] = 0x01
    if ext:
        o[24], o[25], o[27], o[28] = 0xFF, 0x88, 0xFF, 0x80
    return o


def monitor(d):
    last = None
    jog0 = None
    while True:
        r = d.read(64, 100)
        if not r:
            continue
        r = bytes(r[:20])
        if last is not None:
            for (b, m), n in BUTTONS.items():
                if (r[b] ^ last[b]) & m:
                    print(f"{n} {'down' if r[b] & m else 'up'}")
            if r[12:14] != last[12:14]:
                print(f"tempo {struct.unpack('<h', r[12:14])[0]:+d}")
            if r[10] != last[10]:
                print(f"selector {(r[10] - last[10] + 128) % 256 - 128:+d} (counter {r[10]})")
            if r[8] != last[8]:
                print(f"vinyl speed {r[8]}")
        jog = struct.unpack("<H", r[14:16])[0]
        if jog0 is None:
            jog0 = jog
        if last is not None and r[14:16] != last[14:16]:
            print(f"\rjog {(jog - jog0 + 32768) % 65536 - 32768:+6d}  speed {struct.unpack('<H', r[16:18])[0]:5d}"
                  f"  flags {r[7]:#04x}", end="   \n" if r[7] & 0x80 == 0 else "", flush=True)
        last = r


def jog(d):
    print("turn the platter one full turn (use a mark), then Ctrl-C")
    acc, prev = 0, None
    try:
        while True:
            r = d.read(64, 100)
            if not r:
                continue
            j = r[14] | r[15] << 8
            if prev is not None:
                acc += (j - prev + 32768) % 65536 - 32768
            prev = j
            print(f"\r{acc:+7d} HID counts", end="", flush=True)
    except KeyboardInterrupt:
        print(f"\n{abs(acc)} HID counts per turn = {abs(acc) / 3:.0f} bus counts "
              f"(stock MAIN: ~9730; bridge option -t {abs(acc)})")


def lamps(d):
    o = out()
    for b, m, _ in LAMPS:
        o[b] |= m
    send(d, o)
    time.sleep(2)
    for b, m, n in LAMPS:
        o = out()
        o[b] |= m
        print(n)
        send(d, o)
        time.sleep(0.8)
    send(d, out())


def ring(d):
    o = out()
    o[11] = 0xF1  # time, tempo, BPM, track, jog
    o[18], o[20], o[21] = 1, 128, 0
    t0 = time.time()
    i = 0
    try:
        while True:
            t = time.time() - t0
            o[12], o[13], o[14] = int(t // 60), int(t % 60), int(t * 100 % 100)
            o[22:24] = struct.pack("<h", int(500 * ((i // 50) % 3 - 1)))
            o[25] = 1 + i % 135
            o[24] = (i // 8) % 85
            o[26] = i % 101
            o[27] = (i // 3) % 101
            o[28] = 0x80 | (0x01 if o[26] > 90 else 0)
            send(d, o)
            print(f"\rring {o[25]:3d} mark {o[24]:2d} bar {o[26]:3d}% dot {o[27]:3d}%", end="", flush=True)
            i += 1
            time.sleep(0.03)
    except KeyboardInterrupt:
        send(d, out())
        print()


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "monitor"
    fn = {"monitor": monitor, "jog": jog, "lamps": lamps, "ring": ring}.get(cmd)
    if not fn:
        sys.exit(__doc__)
    d = open_dev()
    try:
        fn(d)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
