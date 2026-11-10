#!/usr/bin/env python3
"""Reads the STM32F401 panel-bus adapter (USB CDC) and decodes CDJ-850 panel frames.

usage: sniff.py [--port /dev/cu.usbmodemXXXX] [--seconds N] [--save file.npy] [--all] [--stats] [--master | --slave]

Without --master the adapter only listens (sniffer, native MAIN connected); with --master it is MAIN: drives TSCK + TSO, waits for PNL_XBUSY, sends idle frames.

Default output = only changes (like tools/saleae/diff.py), with checksum check and drop/length errors.
--save writes the same .npy layout as tools/saleae/decode.py: object array of (t_s, TSI bytes, TSO bytes),
so tools/saleae/diff.py and tlv.py work on adapter captures too.
"""
import argparse, glob, struct, sys, time

import numpy as np
import serial

sys.path.insert(0, __file__.rsplit('/', 1)[0])  # tlv.py next to this script
try:
    from tlv import parse as tlv_parse
except Exception:
    tlv_parse = None

REC = 44
SYNC = b'\xa5\x5a'


def cksum(b):
    s = 0
    for x in b:
        s += x
        if s > 0xFF:
            s = (s & 0xFF) + 1
    return s


def ok_tso(b):  # MAIN → panel: byte 15 over 0..14
    return not any(b) or b[0] == 0xFF or cksum(b[:15]) == b[15]


def ok_tsi(b):  # panel → MAIN: type 01 byte 15, type 02 byte 12 over 0..11
    if not any(b) or b[0] == 0xFF:
        return True
    if b[0] == 0x02:
        return cksum(b[:12]) == b[12]
    return cksum(b[:15]) == b[15]


# noisy fields, as in diff.py
TSI_MASK = {1: {5, 6, 7, 8, 9, 10, 11, 12, 15}, 2: {8, 11, 12, 13, 14, 15}}
TSI_LOW = {(1, 13): 0x0F}


def records(port):
    buf = bytearray()
    while True:
        chunk = port.read(max(1, port.in_waiting))
        if not chunk:
            yield None  # idle: lets the caller check its timer
            continue
        buf += chunk
        while True:
            i = buf.find(SYNC)
            if i < 0:
                del buf[:-1]
                break
            if len(buf) - i < REC:
                del buf[:i]
                break
            r = bytes(buf[i:i + REC])
            del buf[:i + REC]
            yield r


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--port')
    ap.add_argument('--seconds', type=float)
    ap.add_argument('--save')
    ap.add_argument('--all', action='store_true', help='print every frame')
    ap.add_argument('--stats', action='store_true', help='only print stats once per second')
    ap.add_argument('--master', action='store_true', help='be MAIN, clocking the bus (MAIN as SPI master)')
    ap.add_argument('--slave', action='store_true', help='be MAIN, answering the panel clock on PB14 (MAIN as SPI slave)')
    a = ap.parse_args()

    port = a.port or next(iter(sorted(glob.glob('/dev/cu.usbmodem*'))), None)
    if not port:
        sys.exit('sniffer not found (/dev/cu.usbmodem*) — is it plugged in and flashed?')
    s = serial.Serial(port, timeout=0.1)
    s.dtr = True
    mode = 1 if a.master else 2 if a.slave else 0
    s.write(bytes([0xA5, 0x5A, 0x03, 1, mode]))  # 0 LISTEN, 1 MASTER, 2 SLAVE
    print('reading', port, file=sys.stderr)

    saved = []
    prev_tsi, prev_tso, text = {}, {}, {}
    last_text = None
    last_seq = None
    t0 = None
    st = dict(frames=0, lost=0, len_err=0, ck_tsi=0, ck_tso=0)
    last_stat = time.time()
    end = time.time() + a.seconds if a.seconds else None
    try:
        for r in records(s):
            if r is None:
                now = time.time()
                if now - last_stat >= 1 and (a.stats or a.seconds):
                    print('[idle] frames %d — no bus clock' % st['frames'], file=sys.stderr)
                    last_stat = now
                if end and now >= end:
                    break
                continue
            seq, t_us, n_o, n_i, flags, dropped = struct.unpack_from('<HIBBBB', r, 2)
            tso, tsi = r[12:28], r[28:44]
            t0 = t_us if t0 is None else t0
            t = ((t_us - t0) & 0xFFFFFFFF) / 1e6
            if last_seq is not None:
                st['lost'] += (seq - last_seq - 1) & 0xFFFF
            last_seq = seq
            st['frames'] += 1
            bad = []
            if n_o != 16 or n_i != 16:
                st['len_err'] += 1
                bad.append('len %d/%d' % (n_o, n_i))
            if not ok_tso(tso):
                st['ck_tso'] += 1
                bad.append('ck TSO')
            if not ok_tsi(tsi):
                st['ck_tsi'] += 1
                bad.append('ck TSI')
            if a.save:
                saved.append((t, tsi, tso, flags))

            if a.stats:
                pass
            elif a.all:
                print('%9.4f %s | %s %s%s' % (t, tsi.hex(' '), tso.hex(' '),
                                             'B' if flags & 1 else '-', ('  !' + ','.join(bad)) if bad else ''))
            else:
                if bad:
                    print('%9.4f  !! %s  TSI %s  TSO %s' % (t, ','.join(bad), tsi.hex(' '), tso.hex(' ')))
                k = tsi[0]
                if k in TSI_MASK:
                    key = tuple(tsi[i] & TSI_LOW.get((k, i), 0xFF) for i in range(1, 16) if i not in TSI_MASK[k])
                    if k in prev_tsi and prev_tsi[k] != key:
                        print('%9.4f  panel→MAIN %02x  %s' % (t, k, ' '.join(
                            ('%02x' % tsi[i]) if i not in TSI_MASK[k] else '..' for i in range(16))))
                    prev_tsi[k] = key
                k = tso[0]
                if any(tso) and k not in (0x60, 0xFF):
                    if k in prev_tso and prev_tso[k] != tso[:15]:
                        print('%9.4f  MAIN→panel %s' % (t, tso.hex(' ')))
                    prev_tso[k] = tso[:15]
                    if 0x81 <= k <= 0x8F and tlv_parse:
                        text[k] = tso
                        rec = tlv_parse(text)
                        txt = ' / '.join('%02x:%s' % (rid, s_) for rid, _, s_ in rec if s_)
                        if txt != last_text:
                            last_text = txt
                            print('%9.4f  TEXT  %s' % (t, txt))

            now = time.time()
            if now - last_stat >= 1 and (a.stats or a.seconds):
                print('[%5.1fs] frames %d  lost %d  len_err %d  cksum TSI %d / TSO %d' % (
                    t, st['frames'], st['lost'], st['len_err'], st['ck_tsi'], st['ck_tso']), file=sys.stderr)
                last_stat = now
            if end and now >= end:
                break
    except KeyboardInterrupt:
        pass
    print('total: frames %d  lost %d  len_err %d  cksum TSI %d / TSO %d' % (
        st['frames'], st['lost'], st['len_err'], st['ck_tsi'], st['ck_tso']), file=sys.stderr)
    if a.save:
        np.save(a.save, np.array(saved, dtype=object), allow_pickle=True)
        print('saved', len(saved), 'frames →', a.save, file=sys.stderr)


if __name__ == '__main__':
    main()
