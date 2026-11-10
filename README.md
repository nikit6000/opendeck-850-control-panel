# opendeck-850-control-panel

Running the **Pioneer CDJ-850 control panel without its MAIN board**. This repository holds:
- the reverse-engineered panel ⇄ MAIN bus protocol;
- the wiring;
- the firmware for a small STM32F401 adapter that takes MAIN's place on the bus.

A host program (Mac, Raspberry Pi, …) talks to the adapter over USB.
- From the panel it gets complete frames: buttons, jog, tempo fader, selector, knobs.
- To the panel it sends lamps, segment display numbers, the jog ring and the cue mark, the position bar and text.

**Status (2026-11-10):** works on a real CDJ-850 with the MAIN board unplugged. Checked so far:
- all buttons, the jog, the fader and every lamp and icon;
- the 7-segment and dot-matrix display;
- the jog ring and the cue mark;
- standby and wake;
- a full player driven through the adapter.

| | |
|---|---|
| [docs/PROTOCOL.md](docs/PROTOCOL.md) | the bus: physical layer, start handshake, frame formats, every known bit (captures + MAIN firmware RE + bit tests) |
| [docs/ADAPTER.md](docs/ADAPTER.md) | the adapter: **wiring**, modes, USB protocol, flashing, how the firmware works |
| [docs/USB-HID.md](docs/USB-HID.md) | the other route: the stock MAIN in PC/LINK mode as a USB HID + MIDI controller (report map) |
| [firmware/](firmware/) | STM32F401 "Black Pill" firmware (Rust, no RTOS) |
| [host/](host/) | Python tools: `sniff.py` (monitor / logger), `text.py` (display text), `sweep.py` `bits.py` `sweepall.py` `cuemark.py` `lightall.py` `sleepbit.py` `sleepgroups.py` (bit/byte probing) |
| [tools/saleae/](tools/saleae/) | decoding Saleae Logic captures of the bus (`decode.py` → frames, `diff.py` → changes) |

## The bus in one paragraph

The panel CPU is the **SPI clock master**. It runs about 1.3 MHz, SPI mode 3, LSB first, and exchanges a 16-byte full-duplex frame every 0.755 ms. MAIN is the slave: it answers on TSO.

The panel starts clocking only after a handshake. Once PWR_ON is up, **MAIN_XBUSY must go high ("booting") and then low ("ready")**. Without that it shows **E-8709**.

Frames 01/02 from MAIN carry the lamps, numbers and indicators. Frames 80 / 81+ / 70–77 carry the dot-matrix screen, its text and the waveform overview. 60 is a heartbeat. Every frame ends in an 8-bit one's-complement checksum.

## MAIN → panel, frames 01 / 02 (summary)

| frame / byte | meaning |
|---|---|
| 01 b1 (= 02 b1) | jog **cue mark**, 0–84 (85 steps per turn); ≥ 85 switches the ring to other modes |
| 01 b2 (= 02 b2) | flags: 0x40 always, 0x18 VINYL, 0x20 jog touch ring |
| 01 b3 (= 02 b3) | jog ring indicator 1–135, 0x88 = off |
| 01 b4 | track number (0xFF blank) |
| 01 b5 / b6 / b7 | time min / s / CD frames (0–74); 0xBB = "–", 0xFF = seeking |
| 01 b8:9 | BPM (integer, big-endian) |
| 01 b10:11 | abs(tempo) |
| 02 b4 | position bar, percent |
| 02 b5 | LEDs: 0x02 USB, 0x04 LINK, 0x08 CUE, 0x10 PLAY, 0x20 LOOP IN, 0x40 LOOP OUT |
| 02 b6 | LEDs: 0x01 RELOOP/EXIT, 0x02 DISC, 0x08 REV, 0x10 MASTER TEMPO, 0x20 VINYL, 0x40 DISC EJECT |
| 02 b7 | LEDs: 0x01–0x08 AUTO LOOP 1–4, 0x10 BROWSE, 0x20 TAG LIST, 0x40 INFO, 0x80 MENU |
| 02 b8 | **0x01 = STANDBY** (the panel switches the player off) |
| 02 b9 | 0x04 track-end warning (position bar blinks) |
| 02 b10 | cue dot on the position bar, percent |
| 02 b11 | 0x03 / 0x02 tempo sign + / −, 0x04 BROWSE, 0x08 TAG LIST, 0x10 MEMORY, 0x80 MT (display icons) |
| 02 b12 | 0x01 A.CUE, 0x08 "CUE", 0x10 / 0x20 tempo decimal point normal / WIDE, 0x40 always, 0x80 INFO |
| 02 b13 | 0x01 REMAIN, 0x0E always, 0x20 time + jog indicator on |
| 02 b14 | 0x80 / 0x40 / 0x20 / 0x04 range ±6 / ±10 / ±16 / WIDE, 0x0A always |

Panel → MAIN, button bits and the rest: see [docs/PROTOCOL.md](docs/PROTOCOL.md).

## Quick start

1. Flash the adapter: `firmware/flash.sh` (ST-Link + probe-rs) or `firmware/flash.sh dfu`.
2. Wire it to CN2007 / MAIN_XBUSY as in [docs/ADAPTER.md](docs/ADAPTER.md). Use 1 kΩ in series on every line, never touch CN2007 pin 1 (+12 V), and unplug the MAIN board.
3. `pip install pyserial numpy`, then `python3 host/sniff.py --slave`. The adapter now plays MAIN, and button presses show up in the terminal.
4. `python3 host/text.py 1 "HELLO" "FROM" "THE HOST"` shows text on the dot-matrix screen.

The Saleae captures and the MAIN firmware image mentioned in PROTOCOL.md are not included.
