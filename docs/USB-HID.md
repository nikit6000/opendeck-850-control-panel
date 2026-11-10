# CDJ-850 USB HID (PC mode) — report map

Player: stock main board, FW MAIN v1.13. Mac ↔ USB-B, LINK (PC) pressed on the player → "PC is connected".
All entries below were checked on the live player on 2026-11-08 unless marked ❓.
Starting point: Mixxx `res/controllers/Pioneer-CDJ-HID.js` (copy in `ref/`; its output part was an unfinished draft).

## USB device
| | |
|---|---|
| VID / PID | `0x08E4` / `0x0157`, "PIONEER Corporation." / "PIONEER CDJ-850", bcdDevice 0x0113, USB 2.0 Full Speed |
| Interfaces | 0 Audio Control, 1 Audio Streaming, 2 "PIONEER CDJ-850 MIDI", 3 "USB MIDI Interface2", **4 "PIONEER CDJ-850 HID"** (2 interrupt EPs, 1 ms) |
| HID report descriptor | `06a0ff 0901 a101 0902 a100 06a1ff 0903 0904 1580 257f 3500 45ff 7508 9514 8102 0905 0906 1580 257f 3500 45ff 7508 9524 9102 c0 c0` |
| Usage page / usage | `0xFFA0` / `0x01` (vendor) |
| Input | 20 bytes, **no report ID** |
| Output | 36 bytes, **no report ID**; byte 0 is a packet type (0x01 = lights) |

The HID descriptor is **not** in MAIN firmware (`fw/out/main.bin`): USB is served by a separate USB/audio IC.
MAIN only has the strings "PC MODE", "NOW PCMODE", "PC is connected.".

Notes:
- Input reports are sent **only on change** (no periodic reports). Every HID button event is mirrored on MIDI too.
- hidapi ≥ 0.12 opens devices **exclusively** on macOS → `cdjhid.py` calls `hid_darwin_set_open_exclusive(0)` so
  several tools can share the player.
- Output: send `b"\x00" + 36 bytes` with hidapi (leading 0 = "no report ID"). The player keeps the last state.

## Input report (20 bytes)
Idle: `00 00 00 00 00 00 80 40 ff 00 0b 00 00 00 80 90 00 00 00 00` (bytes 10, 14:15 are counters, vary).

### Buttons (bit = 1 while held)
| byte | 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20 | 0x40 | 0x80 |
|---|---|---|---|---|---|---|---|---|
| 0 | DISC EJECT | – | TRACK \|◀◀ | TRACK ▶▶\| | SEARCH ◀◀ | SEARCH ▶▶ | CUE | PLAY/PAUSE |
| 1 | – | – | – | – | – | RELOOP/EXIT | LOOP OUT | LOOP IN |
| 2 | – | – | – | TIME MODE | DELETE | CALL ◀ | CALL ▶ | MEMORY |
| 3 | – | VINYL | ❓ (Mixxx: tempo reset) | MASTER TEMPO | TEMPO RANGE | SELECTOR push | BACK | – |
| 4 | – | – | – | BEAT SELECT | AUTO LOOP 1 | AUTO LOOP 2 | AUTO LOOP 3 | AUTO LOOP 4 |
| 5 | – | – | – | – | – | – | – | – |
| 6 | TAG TRACK/REMOVE | – | – | – | – | – | – | idle 1 ❓ |
| 7 | DIRECTION REV | – | – | – | – | JOG TOUCH | jog dir (1 = forward) | jog moving |

### Analog / counters (multi-byte = little-endian)
| bytes | meaning |
|---|---|
| 8 | VINYL SPEED ADJUST knob, u8 (151…254 seen; 0xFF idle position) |
| 9 | ❓ always 0 (Mixxx: "release/start knob" — not on the 850) |
| 10 (11 = 0) | SELECTOR rotary: click counter, +1 CW / −1 CCW; restarts at 11 after re-enumeration → use deltas (mod 256) |
| 12:13 | TEMPO fader, s16: −1000 … 0 … +1000 |
| 14:15 | JOG position, u16 counter: +3 per tick CW, −3 CCW, wraps (use deltas mod 65536) |
| 16:17 | JOG speed, u16, steps of 10 (0 = stopped; direction in byte 7 bit 0x40) |
| 18:19 | ❓ always 0 (Mixxx: needle search — not on the 850) |

### Buttons that are NOT in the HID report (MIDI only)
In PC mode the browser buttons are reported only on the MIDI port "PIONEER CDJ-850" (note on `0x90`, vel 0x7F/0x00):

| note | button |
|---|---|
| 0x36 | BROWSE |
| 0x37 | TAG LIST |
| 0x38 | INFO |
| 0x3F | MENU/UTILITY (opens the player's own menu too) |

Not seen at all (HID or MIDI): USB STOP (dedicated line), source buttons USB / DISC / LINK (not tested — they may leave PC mode).
On opening the MIDI port the player sends a state snapshot (CC `b0 1d xx` tempo, `b0 1e` vinyl speed, `b0 10/30` jog …).

## Output report "lights" (36 bytes, byte 0 = 0x01)
### Lamps (bit = 1 → on)
| byte | 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20 | 0x40 | 0x80 |
|---|---|---|---|---|---|---|---|---|
| 4 | A.CUE (display) | REMAIN (display) | – | RELOOP/EXIT | LOOP OUT | LOOP IN | CUE | PLAY |
| 5 | MEMORY (display) | – | – | WIDE | ±16 % | ±10 % | ±6 % | MASTER TEMPO |
| 6 | AUTO LOOP 1 | AUTO LOOP 2 | AUTO LOOP 3 | AUTO LOOP 4 | REV | jog ring (dashed circle) | jog "VINYL" logo | VINYL button |
| 7 | jog touch highlight | – | – | – | – | – | – | – |
| 8 | BROWSE | – | INFO | – | – | – | MENU | TAG LIST |

"–" = tested, nothing visible (byte 4 0x04; byte 5 0x02/0x04; byte 7 0x02…0x80 = hot-cue LEDs of CDJ-900/2000; byte 8 0x02/0x08/0x10/0x20).
Bytes 9, 10, 19, 24–35 set to 0xFF: no visible change (apart from byte 15 below).

### Display fields
| byte(s) | meaning |
|---|---|
| 11 | field enables; Mixxx uses `0x01 time | 0x10 tempo | 0x20 BPM | 0x40 track | 0x80 jog` = 0xF1 for the 850. 0 → numbers blank |
| 12 / 13 / 14 | time: minutes / seconds / **hundredths** (player shows them as CD frames: 45 → "33 f") |
| 15 | progress / overview bar ❓ — non-zero lights it; partial fills seen, but not repeatable (stateful? maybe built column by column like internal frame 0x70) |
| 16, 17 | ❓ (Mixxx "wave summary 2/3") |
| 18 | track number (2 digits) |
| 20:21 | BPM, u16 LE, integer, last 3 digits shown (1285 → "285") |
| 22:23 | tempo, s16 LE, 0.01 % (250 → "+2.50 %", −1234 → "−12.34 %") |

The jog-display indicator turns with the time value (it moved while minutes changed).

### Text
No text output found: packet types 0x02–0x0F with ASCII, UTF-16LE and Mixxx-style `02`+UTF-16LE bodies show nothing
(`textprobe.py`). In PC mode the dot-matrix text is probably drawn by the player itself.

## Tools (`.venv` = Python 3.9 + hidapi 0.15 + python-rtmidi)
| file | |
|---|---|
| `cdjhid.py` | library: open device, `decode()` input, `CDJ.led()/raw()/time()/track()/bpm()/rate()`, MIDI helper |
| `monitor.py` | live log of presses/releases (HID + MIDI-only buttons), jog Δ / speed, tempo, knobs; `--raw` for bytes |
| `leds.py` | lamps: `leds.py on PLAY CUE`, `off …`, `all`, `none`; no args → REPL (`on/off/t/list/raw/time/track/bpm/rate/sweep`) |
| `learn.py` | guided mapping, one button at a time → `learned_inputs.json` |
| `textprobe.py`, `barsweep.py` | probes used for the display text / bar |

```
cd mac-hid   # tools not included in this repository
.venv/bin/python monitor.py
.venv/bin/python leds.py on PLAY "AUTO LOOP 1" MT
```

## Worn pads (hardware, not protocol)
TIME MODE (late/missed presses), DELETE (ghost MEMORY ~20 ms), BROWSE (ghost TAG LIST), INFO (ghost MENU), DISC EJECT.
Ghosts show up here exactly as on the internal panel bus; debounce ≥ 60 ms in software.

## Open questions
- Byte 15 bar semantics (and bytes 16/17), BPM decimal point, remaining-time mode values.
- Text on the dot-matrix display over USB.
- Input byte 3 0x04, byte 6 0x80 (idle-high), bytes 9, 18:19.
