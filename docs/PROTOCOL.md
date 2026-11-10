# CDJ-850 panel bus (CN1001 / CN2007) — notes

Capture: `captures/boot_play.sal` (20 s, player already on, NO DISC, one button press ~1.63 s).

## Physical layer
- 3 wires + handshake: TSCK, TSI, TSO, PNL_XBUSY, MAIN_XBUSY.
- Names are from the MAIN CPU's point of view:
  - **TSO = MAIN → panel** (contains display text)
  - **TSI = panel → MAIN** (contains ADC values, buttons)
- **The panel is the clock master, MAIN is the SPI slave.** Confirmed 2026-11-10 with the STM32 adapter (this repository), MAIN unplugged:
  - MAIN firmware sets SCIF1 SCSMR = 0x80 (synchronous) and SCSCR = 0x02 (CKE = 10b, external clock).
  - Before each frame MAIN resets the SCIF and loads its 16 bytes into the TX FIFO (0x0C0FA05E).
- **Start handshake: MAIN_XBUSY**, driven by MAIN.
  - After power-on (PWR_ON high), MAIN_XBUSY must be **high ("busy")**, then go **low ("ready")**. Only then does the panel start clocking.
  - A MAIN_XBUSY that is already low at power-on (or never driven) → the panel never clocks → **E-8709**.
  - The adapter holds it high for 2.8 s after PWR_ON (the native MAIN's boot time), then low, and the panel starts at once.
  - The old note "MAIN_XBUSY stayed low the whole capture" is the steady state after this handshake.
- **PNL_XBUSY** (panel → MAIN): rises 4 µs after every third frame and falls ~656 µs later. The next clock burst starts 64 µs after the fall, which is MAIN's time to reload its FIFO.
- Clock: idles HIGH, data changes on the falling edge and is sampled on the rising edge → SPI mode 3.
  - Measured from the captures: 128 bits in 98.9 µs, so **~1.3 MHz**. The earlier 1.6 MHz was a mis-estimate.
  - Period ≈ 0.755 ms per frame.
- **LSB first.**
- Transaction = exactly 128 clocks = **16 bytes each direction**, full duplex, one every ~0.76 ms.

## Frame format (16 bytes)
- byte 0 = frame type
- bytes 1..14 = payload
- byte 15 = checksum = 8-bit one's-complement sum (end-around carry) of bytes 0..14.
  Verified for all MAIN→panel frames and panel type 0x01. Panel type 0x02 carries it in byte 12 (over bytes 0..11).

## MAIN → panel (TSO), types cycle: 01, 02, 60, 80, 81, 82
- 0x60: `60 NN 00..` heartbeat counter NN (+1 every ~20 ms)
- 0x81 / 0x82: display text, e.g. "NO D" + "ISC" = "NO DISC"; 0x82 also contains "terjax"
  - **Text is a record stream** across payloads (bytes 1..14) of 0x81, 0x82, 0x83 … concatenated:
    `[id][3 attr bytes][len][len chars]` repeated (parser: `tlv.py`).
    - no media: `(0x11,"")`, `(0x12,"NO DISC" / "NO USB" / "NOT AVAILABLE")`, `(0x13,"")`
    - CD loaded (`led_base`): `(0x25 attr 01 00 00,"Afrojack")`, `(0x02,"Blasterjaxx")`, `(0x03,"Cat")` (CD-Text artist/title/…)
    - "terjax" seen earlier = stale tail of "Blasterjaxx" left in the buffer from a previously loaded disc.
- 0x01, 0x02, 0x80: unknown (LEDs / display segments / modes?)
- **Text on the dot-matrix display works from our own MAIN** (2026-11-10, `host/text.py`):
  - frame 80 = screen 0x01 (track), with byte 1 = a fresh sequence number;
  - records `0x25` / `0x02` / `0x03` (attr `01 00 00`) in frames 81, 82, …
  - result: our three lines on the screen.

## Panel → MAIN (TSI), types alternate: 01, 02 (all-zero frames in between)
Capture `captures/jog_analog.sal` (45 s): jog CW/CCW 11–19 s, jog touch 20.5–22.5 s, pitch sweep 28–31 s, vinyl speed knob 31.8–36 s, browse encoder 36.5–40 s.

### type 0x01 (16 bytes, checksum byte 15 OK)
| bytes | meaning |
|---|---|
| 1 | 0x01 (button bitmap? see capture 1) |
| 3:4 | **browse/selector encoder**, signed 16-bit BE position (−10..+5 seen) |
| 5:6 | **tempo (pitch) fader**, 16-bit BE: 0x0000 = one end, 0xFFFF = other, ~0x7FC0 centre |
| 7:8 | second 16-bit ADC, stays ~0x7FC0 (ADCT centre reference?) |
| 9:10 | **jog position**, 16-bit BE, + for CW, − for CCW (~0x0D60 counts for roughly one turn) |
| 11:12 | **jog pulse period**, 16-bit BE; 0xFFFF = stopped, ~0x0200–0x0400 at slow turn |
| 13 | **jog flags**: 0x80 = moving, 0x40 = CW direction, 0x30 = top touched (0x70 seen when palm on platter) |

### type 0x02 (only 13 bytes meaningful)
| bytes | meaning |
|---|---|
| 1 | button bitmap (0x02 seen for ~140 ms in capture 1) |
| 8 | **VINYL SPEED ADJUST knob**, 8-bit 0x00..0xF0 |
| 9:10 | constant `06 ff` |
| 11 | echo of MAIN's 0x60 heartbeat counter |
| 12 | **checksum** of bytes 0..11 (same one's-complement sum) |
| 13..15 | `f1 64 76` constant, padding / not covered by checksum |

## Buttons (one capture per action, `./one.sh <name>`)
| button | frame | byte | mask |
|---|---|---|---|
| PLAY/PAUSE | 0x02 | 1 | 0x02 |
| CUE | 0x02 | 1 | 0x04 |
| LOOP IN | 0x02 | 1 | 0x08 |
| LOOP OUT | 0x02 | 1 | 0x10 |
| RELOOP/EXIT | 0x02 | 1 | 0x20 |
| CALL ◀ | 0x02 | 4 | 0x01 |
| CALL ▶ | 0x02 | 4 | 0x02 |
| DELETE | 0x02 | 4 | 0x04 — flaky: during a firm hold flips to 0x08 (= MEMORY, up to 286 ms) and once gave 0x40; pad worn → replace |
| MEMORY | 0x02 | 4 | 0x08 — short ghosts of 0x10 (≤34 ms) at press start; filterable |
| TIME MODE | 0x02 | 5 | 0x01 — confirmed; MAIN toggles indicator on *release* |
| AUTO LOOP 1 | 0x02 | 2 | 0x01 |
| AUTO LOOP 2 | 0x02 | 2 | 0x02 |
| AUTO LOOP 3 | 0x02 | 2 | 0x04 |
| AUTO LOOP 4 | 0x02 | 2 | 0x08 |
| BEAT SELECT | 0x02 | 2 | 0x10 — MAIN type 0x02 byte 7: 0x1f→0x10 on press (auto-loop LEDs / beat-length indicator?) |
| DIRECTION REV | 0x02 | 2 | 0x20 — MAIN type 0x02 byte 6: 0x32→0x3a (+0x08) on press (REV LED / display icon?) |
| DISC EJECT | 0x02 | 3 | 0x01 — "EJECT" text appeared 29 ms after 0x01 began; in that capture a ghost 0x02 (VINYL, 92 ms) came first and toggled vinyl mode OFF |
| VINYL (jog mode) | 0x02 | 3 | 0x02 — clean 180 ms; MAIN 0x02 byte 6 bit 0x20 = VINYL LED (0x1a→0x3a) |
| TEMPO RANGE | 0x02 | 3 | 0x08 — MAIN type 0x02 byte 14: 0x4a→0x2a on press (range indicator bits 0x40→0x20?) |
| MASTER TEMPO | 0x02 | 3 | 0x10 — MAIN type 0x02: byte 6 0x22→0x32 (+0x10) and byte 11 0x04→0x84 (+0x80) on press (button LED + display MT icon?) |
| BACK | 0x02 | 7 | 0x10 |
| TAG TRACK / REMOVE | 0x02 | 7 | 0x20 |
| PC (source) | 0x02 | 6 | 0x01 — MAIN reconfigures display (see note "PC switch") |
| USB (source) | 0x02 | 6 | 0x02 — display text becomes "NO USB" |
| DISC (source) | 0x02 | 6 | 0x04 — MAIN switched to "NO DISC" on it; ghosts of 0x08 (34–75 ms) around the press. 2026-11-10 (adapter): worn pad, a press flips 0x08 ↔ 0x04 (rarely 0x10) the whole time → the correct code is 0x04 (replace the pad) |
| BROWSE | 0x02 | 7 | 0x01 — by elimination (its press swept 0x02 → 0x04 → 0x02 → 0x01); re-test after pad replacement |
| INFO | 0x02 | 7 | 0x04 — press: ghost 0x08 (MENU, 55 ms) then 0x04 dominant (140 ms); MAIN: "NOT AVAILABLE" → "NO DISC" |
| TAG LIST | 0x02 | 7 | 0x02 — MAIN shows "NOT AVAILABLE" (no media), then its 0x80 byte 1 counter runs every ~32 ms (text scroll/blink?) |
| MENU / UTILITY | 0x02 | 7 | 0x08 — release bounce seen (off 9 ms, on 23 ms) |
| USB STOP | 0x02 | 1 | 0x01 — **seen on the bus 2026-11-10 with our adapter as MAIN** (4 presses). With the native MAIN the capture `btn_usb_stop` showed nothing (probably not in USB mode); there is also a separate USB_STOP line, CN2003 pin 11 |
| SELECTOR push | **0x01** | 13 | 0x01 (low nibble; high nibble = jog flags) |
| SEARCH ◀◀ | 0x02 | 5 | 0x08 |
| SEARCH ▶▶ | 0x02 | 5 | 0x10 |
| TRACK SEARCH \|◀◀ | 0x02 | 5 | 0x02 |
| TRACK SEARCH ▶▶\| | 0x02 | 5 | 0x04 |

MAIN reaction to TRACK buttons (no disc): MAIN type 0x01 byte 4 changed 0x00 → 0x63 on \|◀◀, then 0x63 → 0x01 on ▶▶\|.
Hypothesis: **MAIN type 0x01 byte 4 = track number** (binary, 1..99, wraps 1 ↔ 99).

Note on TIME MODE (capture `btn_time_mode2`, 3 presses): one press missing, one gave TRACK ▶▶| bit (0x04, 14 ms),
one gave 0x01 (112 ms) immediately followed by TRACK \|◀◀ bit (0x02, 34 ms). MAIN accepted all of them (track 1→2→1)
and toggled MAIN type 0x02 byte 13 bit 0x01 (0x2e→0x2f; probably ELAPSED/REMAIN indicator).
Hypothesis: byte-5 buttons share one resistor-ladder ADC key line; a worn/dirty TIME MODE contact gives
intermediate voltages that decode as neighbouring buttons. Clean contact and re-test.

After cleaning (`btn_time_mode3`, 3 presses): press 1 → ghost SEARCH ▶▶ (0x10, 20 ms) only; press 2 → ghost 0x04 (50 ms)
then 0x01 (100 ms); press 3 → clean 0x01 (128 ms). MAIN type 0x02 byte 13 toggled 0x2f→0x2e→0x2f, ~6 ms after each
0x01 *release*. Ghosts appear at the start of a press (contact resistance falling through other buttons' windows).
Our firmware should debounce: accept a byte-5 code only after it is stable ≥ 60 ms. Pad still marginal — replace it.

PC switch (`btn_pc`): MAIN type 0x80 byte 1 went 06→07→08 (state/sequence number); panel type 0x02 byte 9
(previously thought constant 0x06) echoes it → **panel 0x02 byte 9 = ack of MAIN 0x80 byte 1**.
MAIN frames changed at once: 0x01/0x02 byte 2 0x58→0x40, byte 3 0x00→0x88; 0x01 byte 4 (track no.) → 0xFF (blank);
0x02 byte 5 0x60→0x64, byte 6 0x32→0x20 (MT 0x10 and REV 0x08 cleared), byte 7 0x10→0x00, byte 11 0x84→0x80;
0x81 byte 1 0x11→0x21, byte 6 0x12→0x22; 0x82 byte 4 0x13→0x23.

BROWSE capture (`btn_browse`): MAIN reacted to each of the three codes:
- after 0x02: text "NOT AVAILABLE" (0x81 byte 10 = 0x0d = 13), MAIN 0x02 byte 7 0x10→0x20, byte 11 0x84→0x88
- after 0x04: text back to "NO DISC", byte 7 → 0x40, byte 12 0x51→0xd1
- after 0x01: byte 7 → 0x10, byte 11 → 0x84
→ MAIN 0x02 byte 7 high nibble looks like the browser-group button LEDs (0x10/0x20/0x40), low nibble = auto-loop LEDs.

EJECT (`btn_eject`): during the ~1 s eject MAIN 0x02 byte 6 toggled 0x1a ↔ 0x5a every ~120 ms → **byte 6 bit 0x40 = a blinking LED/indicator (disc/eject)**; MAIN 0x01/0x02 byte 3 = 0x8a while ejecting.

## LED / state captures with CD loaded
Baseline `led_base` (disc loaded, track 1, paused at cue):
```
MAIN 01: 01 02 58 03 01 04 32 4a 00 ae 00 00 00 00 00 8e
MAIN 02: 02 02 58 03 01 78 62 1f 00 04 01 04 59 2f 4a 36
MAIN 80: 80 03 01 00 01 00 10 00 00 02 00 20 03 00 00 ba
MAIN 83: new text frame type (third record)
```
PLAY (`led_play`, PLAY pressed at 0.714 s):
- MAIN 01: **byte 4 = track, byte 5 = minutes, byte 6 = seconds, byte 7 = CD frames (0..74)** — remaining time (REMAIN mode), counts down 4:50 → 4:41 in 9 s.
- MAIN 01 & 02 **byte 3 = jog-ring indicator position**, wraps ~0..0x87 every ~1.8 s (= 33⅓ rpm) while playing; static when paused.
- MAIN 02 byte 4: slow 0..4 while playing (progress/address bar?).
- MAIN 02 **byte 5 bit 0x10 = PLAY LED** (0x68 → 0x78 at press).
- New frame **0x70** while playing: byte 1 increments every ~3 s, bytes 3,4,5 fill progressively (0b, 06, 08) — BPM analysis / display? TBD.

PAUSE (`led_pause`, pressed at 1.647 s):
- MAIN 01/02 **byte 2: 0x58 playing → 0x78 paused** (bit 0x20 = paused).
- Jog ring (byte 3) stops (stays 0x05).
- MAIN 02 byte 5 while paused: **bit 0x10 (PLAY LED) blinks ~0.6 s on / 0.6 s off**, **bit 0x08 blinks ~0.25/0.25 s** (CUE LED — CDJ blinks CUE in pause). MAIN generates the blinking itself.
- MAIN 02 byte 4 toggles 0x16/0x17 (meaning TBD; was 0..4 during play).
- **0x70 looks like the waveform overview**: `70 16 00 0b 06 08 06 07 0e 0f 0f 0f 0f 0a 04 ..` —
  byte 1 = number of filled columns (0x16 = 22), bytes 3.. = amplitudes 0..0x0f per column, built up while the CD plays.

CUE while paused (`led_cue`, pressed at 2.365 s) — sets a new cue point at the pause position (time stays 3:46.12):
- MAIN 01/02 **byte 1: 0x02 → 0x04** (player state: 02 = play/pause, 04 = at cue); byte 2 0x78 → 0x58.
- MAIN 01 bytes 5–7 = 0xFF 0xFF 0xFF for ~200 ms (time blanked during seek), then time again.
- MAIN 02 byte 5: **bit 0x08 = CUE LED** — solid 100 % at the cue point (blinked in pause); **PLAY (0x10) keeps blinking** (~40–60 % duty).
  bit 0x20 dipped for ~2 s after the press (meaning TBD); bit 0x40 always on.
- MAIN 02 byte 13 0x2f→0x0f→0x2f briefly; byte 10 0x01→0x17.

LOOP (`led_loop`: PLAY 2.39 s, LOOP IN 4.32 s, LOOP OUT 6.40 s, loop ~2.08 s long):
- PLAY from cue: byte 5 bit 0x10 (PLAY) → solid.
- MAIN 01/02 byte 1: 0x04 → 0x0a at LOOP IN.
- **MAIN 02 byte 5 bit 0x20 = LOOP IN LED**, **bit 0x40 = LOOP OUT LED**: both solid normally;
  after LOOP IN → 0x20 blinks; after LOOP OUT (loop active) → 0x20 and 0x40 blink together, toggling every ~0.30 s.
- During the active loop also: **MAIN 02 byte 6 bit 0x01 → on** (LOOP icon on display?), **byte 7 bit 0x08 blinks**
  (= AUTO LOOP 4 LED, the manual loop was ~4 beats — see AUTO LOOP 4 below).
- btn.py's old text parser is obsolete — use tlv.py.

EXIT loop (`led_exit`, RELOOP/EXIT at 1.894 s): LOOP IN (b5 0x20), LOOP OUT (b5 0x40) and AUTO LOOP 4 (b7 0x08) stop blinking → **solid**.
Byte 6 bit 0x01 stays ON and state byte 1 stays 0x0a → both mean "a loop is stored" (RELOOP available), not "loop active".
So: blinking IN/OUT (+ matching AUTO LOOP n) = loop playing; solid = not looping.

AUTO LOOP 1 while playing (`led_autoloop1`, press 1.505 s), tool `ba.py` (before/after duty per bit):
- **MAIN 02 byte 7 bit 0x01 = AUTO LOOP 1 LED** — solid → blinking (50 %) while its auto-loop plays.
- LOOP IN / LOOP OUT (b5 0x20/0x40) blink as in a manual loop.
- State byte 1 (01 and 02): bit 0x02 → 0, bit 0x20 → 1 (0x0a → 0x28): 0x02 manual loop?, 0x20 auto loop?
- MAIN 02 byte 10: bit 0x10 → 0x20 (loop length shown on display?).

AUTO LOOP 2 during auto-loop 1 (`led_autoloop2`): **b7 0x02 = AUTO LOOP 2 LED** → blinking; b7 0x01 (AUTO LOOP 1) back to solid.
Byte 10 and state byte unchanged (so byte 10 is not the loop length).
AUTO LOOP 3 (`led_autoloop3`): **b7 0x04 = AUTO LOOP 3 LED** → blinking; b7 0x02 back to solid.
AUTO LOOP 4 (`led_autoloop4`): **b7 0x08 = AUTO LOOP 4 LED** → blinking (b7 0x04 back to solid). Confirmed visually:
AUTO LOOP 4 + LOOP IN/OUT blink. The earlier "RELOOP/EXIT" attribution was wrong: in the manual ~2.08 s loop
(≈ 4 beats) the player lit the matching AUTO LOOP 4 button. RELOOP/EXIT LED bit: still unknown (maybe no LED).
=> **MAIN 02 byte 7 bits 0x01/0x02/0x04/0x08 = AUTO LOOP 1/2/3/4 LEDs**; blinking = that loop length is active.

BEAT SELECT during auto-loop 4 (`led_beatsel`): b7 0x47/0x4f → 0x40/0x48: AUTO LOOP 1–3 LEDs go OFF, AUTO LOOP 4 keeps
blinking (its loop still plays). No separate "fraction mode" bit found in bytes 6, 7, 10 → mode is shown only by which
auto-loop LEDs are lit (and maybe on the display). b7 0x40 constant in all disc captures (meaning TBD).
Visually confirmed by user: AUTO LOOP 1–3 dark, AUTO LOOP 4 blinking.

MEMORY (`led_memory`, press 4.93 s; again 9.23 s): the disc is an **MP3 data CD** — text record 0x25 holds the file name
"Afrojack - Rock The House (Original Mix).mp3" (44 chars, spans frames 0x81–0x84+).
MEMORY → popup **"NO USB"** for ~2 s (memory cues for this disc need a USB device), then the track screen returns. No cue was stored.
- **MAIN 0x80 = screen/page frame**: byte 1 = sequence number (+1 per screen change, panel echoes it in its 0x02 byte 9),
  **byte 2 = screen id**: 0x08 no media, 0x01 PC, 0x0a track info, 0x07 message popup; bytes 10–13 differ per screen.
- Panel ghost again: byte 4 0x10 (unknown) for 190 ms before MEMORY 0x08.

New cue point (`jog_cue`: PLAY 3.06 s, PAUSE 5.33 s, CUE 6.14 s at ring 0x64):
- No field moved to the new cue position: 01 bytes 8–9 stayed 0x0082; 02 byte 4 kept toggling 0x26/0x27 (~1 s) before and after.
  → the jog-display cue mark has no obvious position field (or it's relative / derived) — needs visual description.
- On CUE: state byte 1 0x28 → 0x3f; byte 2 0x78 → 0x58; CUE LED solid, PLAY blinks, LOOP IN (b5 0x20) starts blinking (~45 %);
  byte 6 bit 0x01 cleared (stored loop forgotten when a new cue is set).
- Time shows 0xFF 0xFF 0xFF for ~0.5 s during the seek.
- User: the cue mark on the jog display is **fixed** (does not rotate) → no position field needed; it is a static segment,
  probably on/off by one bit. Jog display summary: rotating indicator = 01/02 byte 3; VINYL label = 02 byte 6 bit 0x20 (likely,
  shared with the VINYL button LED); separator ring + cue mark = static segments, bits TBD (find by bit sweep with own MAIN).

Boot of the bus partially captured in `disc_check` (bus starts at 2.79 s with 0xFF frames).

## Wiring path panel ↔ MAIN (service manual pp. 16, 76, 78, 102, 104)
DFLB CN1001 (21P) ⇄ FFC (pins reversed: CN1001 = 22 − CN2003) ⇄ CMPX CN2003 (21P) → CMPX → CN2002 (27P) ⇄ FFC DDD1523
(1.0 mm, 70 mm, same face) ⇄ MAIN CN402 (27P).
**CN2002/CN402 (27P)**: 1 GNDD, 2 V+12_EUP, 3 GNDD, 4 GNDD, 5 V+5R6, 6 V+5R6, 7 GNDD, **8 TSCK**, 9 GNDD, **10 PNL_XBUSY**,
**11 TSO**, **12 MAIN_XBUSY**, **13 TSI**, 14 MAIN_XRST, 15 GNDD, **16 MAIN_XMUTE**, 17 GNDD, 18 VDET, 19 GNDD, 20 PWR_ON,
21 GNDD, 22 USB_VBUSON, 23 GNDD, 24 USB_CODETFLG, 25 GNDD, 26 USBA_D_N, 27 USBA_D_P.
**CN2003 (CMPX side, 21P)**: 1 TSO, 2 PNL_XBUSY, 3 TSI, 4 GNDD, 5 TSCK, 6 GNDD, 7 PNL_CNVSS, 8 GNDD, 9 PNL_XRST, 10 MAIN_XBUSY,
11 USB_STOP, 12 MAIN_XRST, 13 PWR_ON, 14 PNL_XMUTE, 15 VDET, 16 GNDD, 17 V+12, 18 V+12, 19 GNDD, 20 V+12_EUP, 21 GNDD.
**CN2007 (CMPX, 9P)**: 1 V+12_EUP (⚠ 12 V), 2 GNDD, 3 PNL_XRST, 4 PNL_CNVSS, 5 TSCK, 6 TSI, 7 PNL_XBUSY, 8 TSO, 9 PWR_ON.
**CN2006/CN403 (17P, DDD1524)**: 1 GNDA, 2 ROUT, 3 GNDA, 4 LOUT, 5 GNDA, 6 GNDD, 7 GNDD, 8 CONT1, 9 GNDD, 10 CONT2, 11 GNDD,
12 USB_D_N, 13 USB_D_P, 14 GNDD, 15 VBUS_B, 16 GNDD, 17 GNDD.
**CN401 (MAIN 13P, from PSU)**: V+10_A, GNDA, V-10_A, V+12, V+7R6_D, V+7R6_M … (power only).
PNL_XRST, PNL_CNVSS, USB_STOP, PNL_XMUTE do **not** reach MAIN — they stay on CMPX/DFLB (panel reset IC1004 BD45302G on DFLB).
FW: SCIF1 SCSMR = 0x80 (sync), SCSCR CKE = 10b (external clock) → **panel is the clock master**, MAIN is the SPI slave. Verified on hardware (see Physical layer, MAIN_XBUSY handshake).

## Firmware C850MAIN.UPD (MAIN v1.13) — verification
Layout (`fw/raw/C850MAIN.UPD`, 2 156 300 B, big-endian SH-2A, NOT encrypted):
- 0x000000: small boot stub (vectors: PC 0x100, SP 0xFFF90000), rest of first 128 KB zeros
- 0x020000: u32 size 0x1EC37 + LZSS stream → `fw/out/loader.bin`
- 0x060000: "CDJ-850 MAIN", "1.13", 0x060020: u32 size 0x19E6E6 + LZSS → `fw/out/main.bin` (loads at 0x0C000000, SDRAM)
- LZSS = Okumura (N=4096, F=18, threshold 2, flag byte LSB-first, 1 = literal, window pre-filled with spaces, r = 0xFEE).
  Decompressor: `fwtools/lzss.py`; disassembly helper: `fwtools/shdis.py` (capstone SH2A BE).

**Key table** at main.bin 0x194EA4 (38 × 8 bytes: u16 word, u16 mask, u32 key id) + names at 0x194FDC (36-byte records)
— used by the debug shell commands KI/KO/KY (emulate key press/release). Word w: high byte = panel 0x02 byte 2w−1,
low byte = byte 2w; w = 0 means panel 0x01 byte 13. **All 33 buttons we mapped from captures match exactly.** New from FW:
| key | panel frame | byte | bit |
|---|---|---|---|
| USB Stop | 0x02 | 1 | 0x01 ✅ (confirmed 2026-11-10 with the adapter) |
| LINK (= the button labelled "PC") | 0x02 | 6 | 0x01 |
| JOG Touch | 0x01 | 13 | 0x20 |
| JOG Position (ON = FWD) | 0x01 | 13 | 0x40 |
| JOG Move | 0x01 | 13 | 0x80 |
Not in the table → unused windows: 0x02 byte 4 bit 0x10, byte 6 bit 0x08 (the "ghost" codes we saw).
Key ids 0x0d..0x32 (Play = 0x0d …). Message strings (16-byte records) at ~0x1544C8: EJECT, NO DISC, LOAD IN, END,
TEST MODE, NO TRACK, NO MEMORY, NO USB MEM, MASTER BPM, NOT AVAILABLE …; task table entry DisplayTask = 0x0C061454.
No named LED table and no LED debug command in the firmware → LED bits recovered by code RE (below).

### Panel driver in MAIN firmware (RE)
- **SCIF1** (0xFFFE8800, accessed via movi20 sign-extended) in clock-synchronous mode; init at 0x0C0F9F14.
- TX: 0x0C0FA05E — double buffer 0x0C9D07A8 (2 × 16 B, toggled by flag at +32), checksum via **0x0C0F9EE8**
  (sum of 15 bytes, `if sum > 0xFF: sum += 1; sum &= 0xFF` — confirms our one's-complement formula), writes 16 B to SCFTDR.
- RX: buffer 0x0C9D... → 0x0CE6C960 holds **two panel frames back-to-back** (0x01 at +0, 0x02 at +16);
  0x0C0D60BC unpacks them into 16-bit words at 0x0CE6C980: words 0–6 = frame 01 bytes 1–14, words 7–12 = frame 02 bytes 1–12.
  Key word array = 0x0CE6C98C (= word 6) → this is the `w` of the key table (w=0 → frame 01 bytes 13/14).
- **Frame 01/02 builder 0x0C065D9E** from TX state struct **S = 0x0C9D0AD8**:
  01: b1..b7 = S[0..6]; b8:9 = S.w8; b10:11 = S.w10; b12 = 0; b13:14 = S.w12.
  02: b1 = S[14], b2 = S[15], b3 = S[16], **b4 = S[20], b5 = S[17], b6 = S[18], b7 = S[19]**, b8..b14 = S[21..27].
- **LED composer 0x0C067F1A**: copies logical flags from L = 0x0C9D2310 (itself a 200-byte copy of player state 0x0CEE506C,
  made by 0x0C10BA20) into S with bld/bst. Called from 0x0C067E40 unless L[7] bit 2 is set (then 0x0C071910 instead).

| L bit | → frame 02 bit(s) | meaning (FW + captures) |
|---|---|---|
| L0.7 | b5 0x10 | PLAY LED ✅ |
| L0.6 | b5 0x08 + b12 0x08 | CUE LED ✅ (+ twin display/jog segment, likely the jog cue mark) |
| L0.5 | b5 0x20 | LOOP IN LED ✅ |
| L0.4 | b5 0x40 | LOOP OUT LED ✅ |
| L0.3 | b6 0x01 | in the L0 transport group next to IN/OUT → **RELOOP/EXIT LED** 🟡 (lit while a loop is stored) |
| L0.1 | b13 0x01 | REMAIN indicator 🟡 |
| L0.0 | b12 0x01 | ? |
| L1.7 | b6 0x10 | MASTER TEMPO ✅ |
| L1.6 / L1.5 / L1.4 | b14 0x80 / 0x40 / 0x20+0x04 | tempo-range indicator segments 🟡 |
| L1.0 | b11 0x10 | ? |
| L2 & 0xC0 | b6 0x20 + frame01/02 b2 0x18 | VINYL (LED + jog "VINYL" + mode flags) ✅ |
| L2.5 | frame01/02 b2 0x20 | PAUSE flag ✅ |
| L2.4 | b6 0x08 | REV ✅ |
| L2.3..0 | b7 0x08..0x01 | AUTO LOOP 4..1 ✅ |
| L4.0 | b7 0x10 + b11 0x04 | browser/source group LED + display icon 🟡 |
| L4.7 | b7 0x20 + b11 0x08 | 〃 🟡 (0x20 seen with "NOT AVAILABLE"/TAG LIST) |
| L4.2 | b7 0x40 + b12 0x80 | 〃 🟡 (0x40 lit with a disc loaded) |
| L4.6 | b7 0x80 | ? |
| L7.6, L1 bits 1–3, L0.2 | — | not used by the composer |
| always | frame01/02 b2 0x40, b14 0x02 | constant |
### Two LED/display composers (DisplayTask main loop 0x0C06149C, every 20 ms)
Flag 0x0C1E109F selects the composer. It is set at 0x0C0E8498 when a **track is loaded** (0x0CE6D267..69 ≠ 0xFF) and cleared
at 0x0C0EF3BE on unload.
- track loaded → **0x0C067E40**: L = copy of P (0x0CEE506C), whose first 20 bytes come verbatim from a PlayerControl
  message (memcpy at 0x0C10B85C); composer 0x0C067F1A (table above) + helpers 0x0C0680C0..0x0C0684DE.
- no track (NO DISC / NO USB / LINK idle) → **0x0C062F80**: clears b5,b6,b7,b12,b13,b15(?) then sets bits from DisplayTask
  globals (0x0CE6D1xx). It always lights LOOP IN/OUT (b5 0x60) — matches our no-media captures.
Source variable 0x0CE6D252: 0/1 = DISC, 2 = USB, 3 = LINK.

### Final bit map (FW + captures)
Frame 02 (MAIN → panel):
| byte | bit | meaning | status |
|---|---|---|---|
| 2 | 0x40 | always on | ✅ |
| 2 | 0x18 | VINYL mode (also in frame 01 byte 2) | ✅ |
| 2 | 0x20 | PAUSE (also frame 01 byte 2) | ✅ |
| 5 | 0x02 | **USB button LED** (bits.py, 2026-11-10; MAIN blinks it on timer 0x0C9D22F0 while source = USB) | ✅ |
| 5 | 0x04 | **LINK source LED** (local composer: source 3) — seen 0x64 in PC capture | ✅ |
| 5 | 0x08 | CUE | ✅ |
| 5 | 0x10 | PLAY | ✅ |
| 5 | 0x20 / 0x40 | LOOP IN / LOOP OUT | ✅ |
| 5 | 0x80 | **USB source LED** — seen 0xe0 in USB capture | ✅ |
| 6 | 0x01 | **RELOOP/EXIT LED** (bits.py) | ✅ |
| 6 | 0x02 | **DISC source LED** (local: source 0/1) — 0x3a in NO DISC, 0x62 with disc | ✅ |
| 6 | 0x08 | REV | ✅ |
| 6 | 0x10 | MASTER TEMPO | ✅ |
| 6 | 0x20 | VINYL | ✅ |
| 6 | 0x40 | **DISC EJECT LED** (bits.py; MAIN blinks it during EJECT) | ✅ |
| 7 | 0x01–0x08 | AUTO LOOP 1–4 | ✅ |
| 7 | 0x10 / 0x20 / 0x40 | **BROWSE / TAG LIST / INFO** button LEDs (active screen) — matches btn_browse capture order | ✅ |
| 7 | 0x80 | **MENU button LED** (bits.py) | ✅ |
| 8 | 0x01 | **STANDBY: the panel switches the player off** (PWR_ON drops, bus stops, STANDBY LED lit) — found 2026-11-10 with sleepbit.py / sleepgroups.py; bits 0x02..0x80 no visible effect | ✅ |
| 9 | 0x04 | **track-end warning: the display position bar blinks** (bits.py); 0x01 / 0x02 no visible effect | ✅ |
| 4 | (byte) | **play position on the display bar, percent 0..100** (native: rises 0→4 in the first 9 s of a ~5 min track, ~22 in mid-track pause) | 🟡 |
| 10 | (byte) | **cue dot on the display position bar, percent 0 = start … 100 = end** — found with `host/sweep.py` on the panel without MAIN (2026-11-10); does NOT move the jog cue mark | ✅ |
| 11 | 0x01 + 0x02 | tempo sign: both = "+", 0x02 only = "−", none = 0 | ✅ |
| 11 | 0x04 / 0x08 | **BROWSE / TAG LIST on the display** (bits.py) | ✅ |
| 11 | 0x10 | **MEMORY on the display** (bits.py) | ✅ |
| 11 | 0x80 | **MT icon on display** — flag 0x0CE6D0E0 = MASTER TEMPO (also drives b6 0x10; seen together in btn_master_tempo) | ✅ |
| 12 | 0x01 | **A.CUE icon** — flag 0x0CE6D0DF = AUTO CUE (persistent setting, default 0, used by playback code) | ✅ |
| 12 | 0x08 | **"CUE" on the display** (bits.py) — not the jog mark (that is byte 1) | ✅ |
| 12 | 0x10 / 0x20 | tempo decimal point: normal / WIDE | ✅ |
| 12 | 0x40 | always on | ✅ |
| 12 | 0x80 | **INFO on the display** (bits.py) | ✅ |
| 13 | 0x01 | REMAIN (MAIN then sends remaining = total − elapsed) | ✅ |
| 13 | 0x0e | always on (time-display segments) | ✅ |
| 13 | 0x20 | **jog indicator / time display enable** — set where ring = (pos mod 135)+1 is written; local path clears it, writes time 0xBB ("--") and ring 0x88 (off) | 🟡 |
| 14 | 0x80 / 0x40 / 0x20 / 0x04 | tempo range ±6 / ±10 / ±16 / WIDE (range stored as steps: 300 = ±6/0.02, 200 = ±10/0.05, 320 = ±16/0.05) | ✅ |
| 14 | 0x02, 0x08 | always on | ✅ |
Settings flags (defaults from factory-reset code 0x0C0E4EB0, restored at boot 0x0C0E8C0C): 0x0CE6D0DD TIME MODE (1 = REMAIN), 0x0CE6D0DE VINYL (1), 0x0CE6D0DF AUTO CUE (0), 0x0CE6D0E0 MASTER TEMPO (0), 0x0CE6D0E2 tempo-range index (3).
Jog ring value: 1..135 = position, 0x88 = off. Time byte 0xBB = blank "-".
Still unknown: b11 0x04/0x08 and b12 0x80 (twins of BROWSE/TAG LIST/INFO LEDs, track-loaded path only), b11 0x10 (L1.0).

**Jog cue mark = frames 01/02 byte 1** (the old "player state"), found 2026-11-10 on the panel without MAIN:
**85 steps per revolution** (0..84). Values ≥ 85 switch the ring into other modes (whole ring lit, indicator drifting).
In the native captures byte 1 changes exactly when a cue / loop point is set, and ≈ ring (byte 3, 135 steps) × 85/135 at
that moment: ring 3 → 2 (track start), 5 → 4 (CUE), 15 → 10 (LOOP IN), 64 → 40 (AUTO LOOP), 100 → 63 (CUE).
The earlier codes "0x02 play/pause, 0x04 at cue, 0x0a loop, 0x28 auto loop" were just mark positions.

Frame 01 (MAIN → panel): **bytes 8:9 = BPM (integer, = L.w16/10 rounded)** (0x0082 = 130); **bytes 10:11 = |tempo| value**
(S.w10, sign in frame 02 byte 11); bytes 5–7 = time (remaining if REMAIN).

Bits set outside the composer (overrides, e.g. blinking/EJECT): b5 0x02/0x04/0x80/0x01, b6 0x02, b6 0x40 (EJECT, set/clr funcs
0x0C063666 / 0x0C063676), loop blinking at 0x0C063000–0x0C0632A0. Raw inventory: `fw/out/bitops_all.txt`, `fw/out/led_inventory.txt`.

## Service modes (service manual RRV4128, §6, ManualsLib pp. 35–40)
| power ON while holding | mode |
|---|---|
| TEMPO (RANGE) + BACK | SERVICE MODE: button name on BROWSE display, LEDs light while pressed, JOG/encoder/volume bars; TAG TRACK = next display pattern, BACK = previous |
| (hold RELOOP ≥10 s) | version / error history (VINYL toggles, CALL ◀/▶ pages) |
| TIME + CD SELECT | CD drive diagnosis (MASTER TEMPO → test operation mode) |
| USB SELECT + RELOOP | ⚠️ firmware update from USB |
| CD SELECT + RELOOP | ⚠️ firmware update from CD |
| BACK + USB SELECT | ⚠️ factory reset |
In SERVICE MODE: turning VINYL SPEED ADJUST fully right enters JOG load measurement.
**Observed (`svc_idle`): in SERVICE MODE the bus is completely silent** (TSCK/TSI/TSO idle high, no transitions for 10 s).
The panel CPU runs the button/LED/display test on its own → it cannot be used to map MAIN→panel LED bits, but it is a
MAIN-independent check of the panel (button names decoded by the panel CPU itself).

Visual service-mode check (user):
- TIME MODE: name conflicts with TRACK ◀ / TRACK ▶ → confirmed worn pad on the byte-5 ladder (hardware, not protocol).
- DELETE: conflicts with MEMORY → confirmed worn pad on the byte-4 ladder; matches capture (0x04 ↔ 0x08).
- DISC EJECT: conflicts with VINYL → worn pad; matches capture (EJECT 0x01 preceded by ghost VINYL 0x02, same byte 3,
  which really toggled vinyl mode OFF). (User's "disk" = DISC EJECT.)
- DISC (source select): service mode shows only "DISC", no conflict. The byte 6 0x08 ghost seen on the bus is unexplained —
  possibly a ladder window for a source button that exists on related models (e.g. LINK) but not on the CDJ-850. Treat 0x08 as unused.
- BROWSE: conflicts with TAG LIST → worn pad; matches capture (BROWSE press showed TAG LIST 0x02 for 28 + 218 ms).
- INFO: conflicts with MENU → worn pad; matches capture (ghost 0x08 MENU before INFO 0x04).
Error E-8709 = COMMUNICATION ERROR panel CPU ↔ main CPU (expect this when emulating MAIN badly).

## TODO
- Captures: each button separately, jog CW/CCW, jog touch, pitch fader sweep, vinyl speed knob, power-on.
- ~~Find who is clock master~~ → the panel; MAIN_XBUSY high→low after PWR_ON starts the bus (2026-11-10).
