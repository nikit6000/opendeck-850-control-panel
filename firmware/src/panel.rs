//! Panel codec: CDJ-850 panel-bus frames <-> the CDJ-850 PC-mode USB HID reports (plus the STM32
//! extensions of `buildroot/docs/PANEL-HID.md`). Pure logic, no hardware: host-testable with
//! `rustc --edition 2021 --test src/panel.rs`.
//!
//! Panel -> host: TSI frames 0x01 (selector, tempo, jog, SELECTOR push) and 0x02 (buttons, VINYL SPEED)
//! become the 20-byte input report. Host -> panel: the 36-byte output report ("lights", byte 0 = 0x01)
//! becomes MAIN frames 0x01 / 0x02 (lamps, 7-segment fields, jog display). Bit maps: docs/PROTOCOL.md
//! (bus) and docs/USB-HID.md (HID).

pub const IN_LEN: usize = 20;
pub const OUT_LEN: usize = 36;
pub type F = [u8; 16];

/// 8-bit one's-complement sum (end-around carry), as MAIN fw 0x0C0F9EE8.
pub fn checksum(b: &[u8]) -> u8 {
    let mut s: u32 = 0;
    for &x in b {
        s += x as u32;
        if s > 0xFF {
            s = (s & 0xFF) + 1;
        }
    }
    s as u8
}

/// Panel frame 0x02 (byte, mask) -> HID input (byte, mask). DIRECTION REV, PC/USB/DISC and the
/// extension byte 5 included.
const KEYS: [(usize, u8, usize, u8); 34] = [
    (1, 0x01, 5, 0x10), // USB STOP          -> ext
    (1, 0x02, 0, 0x80), // PLAY/PAUSE
    (1, 0x04, 0, 0x40), // CUE
    (1, 0x08, 1, 0x80), // LOOP IN
    (1, 0x10, 1, 0x40), // LOOP OUT
    (1, 0x20, 1, 0x20), // RELOOP/EXIT
    (2, 0x01, 4, 0x10), // AUTO LOOP 1
    (2, 0x02, 4, 0x20), // AUTO LOOP 2
    (2, 0x04, 4, 0x40), // AUTO LOOP 3
    (2, 0x08, 4, 0x80), // AUTO LOOP 4
    (2, 0x10, 4, 0x08), // BEAT SELECT
    (2, 0x20, 7, 0x01), // DIRECTION REV (a push button: bit = held, as the stock MAIN)
    (3, 0x01, 0, 0x01), // DISC EJECT
    (3, 0x02, 3, 0x02), // VINYL
    (3, 0x08, 3, 0x10), // TEMPO RANGE
    (3, 0x10, 3, 0x08), // MASTER TEMPO
    (4, 0x01, 2, 0x20), // CALL <
    (4, 0x02, 2, 0x40), // CALL >
    (4, 0x04, 2, 0x10), // DELETE
    (4, 0x08, 2, 0x80), // MEMORY
    (5, 0x01, 2, 0x08), // TIME MODE
    (5, 0x02, 0, 0x04), // TRACK |<<
    (5, 0x04, 0, 0x08), // TRACK >>|
    (5, 0x08, 0, 0x10), // SEARCH <<
    (5, 0x10, 0, 0x20), // SEARCH >>
    (6, 0x01, 5, 0x20), // PC (LINK)         -> ext
    (6, 0x02, 5, 0x40), // USB (source)      -> ext
    (6, 0x04, 5, 0x80), // DISC (source)     -> ext
    (7, 0x01, 5, 0x01), // BROWSE            -> ext
    (7, 0x02, 5, 0x02), // TAG LIST          -> ext
    (7, 0x04, 5, 0x04), // INFO              -> ext
    (7, 0x08, 5, 0x08), // MENU/UTILITY      -> ext
    (7, 0x10, 3, 0x40), // BACK
    (7, 0x20, 6, 0x01), // TAG TRACK/REMOVE
];

/// Debounce per frame-0x02 byte, ms. Bytes 3..7 are resistor ladders whose worn pads give short
/// "ghost" codes of a neighbour at the start of a press (PROTOCOL.md): a new value counts only after it
/// has been stable this long. Byte 1 (PLAY, CUE, loop) and byte 2 (AUTO LOOP, BEAT SELECT, REV) are
/// instant.
const HOLD_MS: [u32; 8] = [0, 0, 0, 60, 60, 60, 60, 60];

/// Bus jog counts -> HID jog counts. The stock MAIN reports ~9730 per turn in steps of 3; the bus
/// counter is ~0x0D60 per turn, i.e. one count per encoder tick. To be confirmed on hardware.
pub const JOG_HID_PER_BUS: i32 = 3;

/// Tempo fader ADC at the centre detent (~0x7FC0) and the dead zone around it.
const TEMPO_CENTRE: i32 = 0x7FC0;
const TEMPO_DEAD: i32 = 48;
/// ADC jitter: a new fader value is accepted only beyond this many counts (1 HID unit = ~33 counts).
const TEMPO_HYST: i32 = 24;

pub struct Decoder {
    raw: [u8; 8],
    since: [u32; 8],
    stable: [u8; 8],
    push: bool,
    jog_flags: u8,
    jog_bus: Option<u16>,
    jog_hid: u16,
    jog_period: u16,
    sel_bus: Option<u16>,
    sel_hid: u8,
    tempo_adc: Option<i32>,
    tempo: i16,
    vinyl: u8,
}

impl Decoder {
    pub const fn new() -> Self {
        Decoder {
            raw: [0; 8],
            since: [0; 8],
            stable: [0; 8],
            push: false,
            jog_flags: 0,
            jog_bus: None,
            jog_hid: 0x9080, // the stock MAIN's idle value
            jog_period: 0xFFFF,
            sel_bus: None,
            sel_hid: 11, // the stock MAIN starts its click counter at 11
            tempo_adc: None,
            tempo: 0,
            vinyl: 0xFF,
        }
    }

    /// Feed one panel (TSI) frame. Returns false for frames that are not 0x01 / 0x02 or fail the checksum.
    pub fn feed(&mut self, f: &F, now_ms: u32) -> bool {
        match f[0] {
            0x01 => {
                if checksum(&f[..15]) != f[15] {
                    return false;
                }
                let sel = u16::from_be_bytes([f[3], f[4]]);
                if let Some(p) = self.sel_bus {
                    self.sel_hid = self.sel_hid.wrapping_add(sel.wrapping_sub(p) as u8);
                }
                self.sel_bus = Some(sel);
                self.tempo_adc_feed(u16::from_be_bytes([f[5], f[6]]) as i32);
                let jog = u16::from_be_bytes([f[9], f[10]]);
                if let Some(p) = self.jog_bus {
                    let d = jog.wrapping_sub(p) as i16 as i32;
                    self.jog_hid = self.jog_hid.wrapping_add((d * JOG_HID_PER_BUS) as u16);
                }
                self.jog_bus = Some(jog);
                self.jog_period = u16::from_be_bytes([f[11], f[12]]);
                self.jog_flags = f[13] & 0xE0; // 0x80 moving, 0x40 CW, 0x20 touch (= HID byte 7)
                self.push = f[13] & 0x01 != 0;
                true
            }
            0x02 => {
                if checksum(&f[..12]) != f[12] {
                    return false;
                }
                for i in 1..8 {
                    if f[i] != self.raw[i] {
                        self.raw[i] = f[i];
                        self.since[i] = now_ms;
                    }
                }
                self.vinyl = ((f[8] as u32 * 17 / 16).min(255)) as u8; // 0..0xF0 -> 0..0xFF
                self.settle(now_ms);
                true
            }
            _ => false,
        }
    }

    /// Accept ladder values that have been stable long enough (call regularly, frames or not).
    pub fn settle(&mut self, now_ms: u32) {
        for i in 1..8 {
            if self.stable[i] != self.raw[i] && now_ms.wrapping_sub(self.since[i]) >= HOLD_MS[i] {
                self.stable[i] = self.raw[i];
            }
        }
    }

    fn tempo_adc_feed(&mut self, t: i32) {
        let accept = match self.tempo_adc {
            None => true,
            Some(p) => (t - p).abs() > TEMPO_HYST || t < TEMPO_HYST || t > 0xFFFF - TEMPO_HYST,
        };
        if !accept {
            return;
        }
        self.tempo_adc = Some(t);
        // the fader towards the player (bottom, "+") gives small ADC values (pi-player, on hardware)
        let d = TEMPO_CENTRE - t;
        self.tempo = if d.abs() <= TEMPO_DEAD {
            0
        } else {
            let span = if d > 0 { TEMPO_CENTRE - TEMPO_DEAD } else { 0xFFFF - TEMPO_CENTRE - TEMPO_DEAD };
            let x = d - d.signum() * TEMPO_DEAD;
            ((x * 1000 + d.signum() * span / 2) / span).clamp(-1000, 1000) as i16
        };
    }

    /// The 20-byte input report (CDJ-850 PC-mode layout, byte 5 = STM32 extension).
    pub fn report(&self) -> [u8; IN_LEN] {
        let mut r = [0u8; IN_LEN];
        for &(fb, fm, hb, hm) in KEYS.iter() {
            if self.stable[fb] & fm != 0 {
                r[hb] |= hm;
            }
        }
        if self.push {
            r[3] |= 0x20;
        }
        r[6] |= 0x80; // idle-high bit of the stock MAIN
        r[7] |= self.jog_flags;
        r[8] = self.vinyl;
        r[10] = self.sel_hid;
        r[12..14].copy_from_slice(&self.tempo.to_le_bytes());
        r[14..16].copy_from_slice(&self.jog_hid.to_le_bytes());
        // jog speed: the stock MAIN steps it by 10, 0 = stopped; here ~ 1 / pulse period
        let speed: u16 = if self.jog_flags & 0x80 == 0 || self.jog_period == 0xFFFF || self.jog_period == 0 {
            0
        } else {
            ((200_000 / self.jog_period as u32).min(6553) * 10) as u16
        };
        r[16..18].copy_from_slice(&speed.to_le_bytes());
        r
    }
}

/// Output report -> MAIN frames 0x01 / 0x02. Returns None for packets that are not "lights" (byte 0 = 1).
pub fn encode(o: &[u8; OUT_LEN]) -> Option<(F, F)> {
    if o[0] != 0x01 {
        return None;
    }
    let mut a = [0u8; 16];
    let mut b = [0u8; 16];
    a[0] = 0x01;
    b[0] = 0x02;
    let bit = |c: bool, m: u8| if c { m } else { 0 };
    let en = o[11];
    let (show_time, show_tempo, show_bpm, show_track, show_jog) =
        (en & 0x01 != 0, en & 0x10 != 0, en & 0x20 != 0, en & 0x40 != 0, en & 0x80 != 0);
    let remain = o[4] & 0x02 != 0;
    let ext = o[28] & 0x80 != 0;

    // time: the stock MAIN wants REMAIN inverted (0x77 - min, 0x8D - s, 0x79 - 1/100 s); the bus takes it plain
    let (mut m, mut s, mut h) = (o[12], o[13], o[14]);
    if remain {
        m = 0x77u8.wrapping_sub(m);
        s = 0x8Du8.wrapping_sub(s);
        h = 0x79u8.wrapping_sub(h);
    }
    let time_ok = show_time && m <= 99 && s < 60 && h < 100;
    if time_ok {
        a[5] = m;
        a[6] = s;
        a[7] = (h as u16 * 75 / 100) as u8; // CD frames
    } else {
        a[5] = 0xBB;
        a[6] = 0xBB;
        a[7] = 0xBB;
    }

    // jog display: ring 1..135 (0x88 off) and cue mark 0..84
    let ring = if ext {
        o[25]
    } else if show_jog && time_ok {
        // like the stock MAIN: the ring turns with the time value, 1.8 s (33 1/3 rpm) per turn
        let hs = (m as u32 * 60 + s as u32) * 100 + h as u32;
        let t = if remain { 10_000_000 - hs } else { hs };
        ((t % 180) * 135 / 180 + 1) as u8
    } else {
        0x88
    };
    let cue = if ext && o[24] < 85 { o[24] } else { 0 };
    a[1] = cue;
    b[1] = cue;
    let vinyl = o[6] & 0xC0 != 0;
    let flags = 0x40 | bit(o[7] & 0x01 != 0, 0x20) | bit(vinyl, 0x18);
    a[2] = flags;
    b[2] = flags;
    a[3] = ring;
    b[3] = ring;
    a[4] = if show_track { o[18] } else { 0xFF };
    let bpm = if show_bpm { u16::from_le_bytes([o[20], o[21]]) } else { 0 };
    a[8..10].copy_from_slice(&bpm.to_be_bytes());
    let tempo = i16::from_le_bytes([o[22], o[23]]);
    let tempo_abs = if show_tempo { tempo.unsigned_abs() } else { 0 };
    a[10..12].copy_from_slice(&tempo_abs.to_be_bytes());

    let src = if ext { o[29] } else { 0 };
    b[4] = if ext { o[26].min(100) } else { 0 };
    b[5] = bit(src & 0x01 != 0, 0x02)      // USB button LED
        | bit(src & 0x02 != 0, 0x04)        // LINK LED
        | bit(o[4] & 0x40 != 0, 0x08)       // CUE
        | bit(o[4] & 0x80 != 0, 0x10)       // PLAY
        | bit(o[4] & 0x20 != 0, 0x20)       // LOOP IN
        | bit(o[4] & 0x10 != 0, 0x40)       // LOOP OUT
        | bit(src & 0x10 != 0, 0x80); //       USB source LED
    b[6] = bit(o[4] & 0x08 != 0, 0x01)      // RELOOP/EXIT
        | bit(src & 0x04 != 0, 0x02)        // DISC LED
        | bit(o[6] & 0x10 != 0, 0x08)       // REV
        | bit(o[5] & 0x80 != 0, 0x10)       // MASTER TEMPO
        | bit(vinyl, 0x20)                  // VINYL
        | bit(src & 0x08 != 0, 0x40); //       DISC EJECT
    b[7] = (o[6] & 0x0F)                    // AUTO LOOP 1..4
        | bit(o[8] & 0x01 != 0, 0x10)       // BROWSE
        | bit(o[8] & 0x80 != 0, 0x20)       // TAG LIST
        | bit(o[8] & 0x04 != 0, 0x40)       // INFO
        | bit(o[8] & 0x40 != 0, 0x80); //      MENU
    b[9] = bit(ext && o[28] & 0x01 != 0, 0x04); // track end: position bar blinks
    b[10] = if ext && o[27] != 0xFF { o[27].min(100) } else { 0 };
    b[11] = bit(show_tempo && tempo > 0, 0x03)
        | bit(show_tempo && tempo < 0, 0x02)
        | bit(o[8] & 0x01 != 0, 0x04)       // BROWSE on the display
        | bit(o[8] & 0x80 != 0, 0x08)       // TAG LIST on the display
        | bit(o[5] & 0x01 != 0, 0x10)       // MEMORY on the display
        | bit(o[5] & 0x80 != 0, 0x80); //      MT on the display
    let wide = o[5] & 0x08 != 0;
    b[12] = 0x40
        | bit(o[4] & 0x01 != 0, 0x01)       // A.CUE
        | bit(ext && o[24] < 85, 0x08)      // "CUE" on the display
        | if wide { 0x20 } else { 0x10 }    // tempo decimal point
        | bit(o[8] & 0x04 != 0, 0x80); //      INFO on the display
    b[13] = 0x0E | bit(remain, 0x01) | bit(time_ok || ring != 0x88 || o[6] & 0x20 != 0, 0x20);
    b[14] = 0x0A
        | bit(o[5] & 0x40 != 0, 0x80)       // +-6
        | bit(o[5] & 0x20 != 0, 0x40)       // +-10
        | bit(o[5] & 0x10 != 0, 0x20)       // +-16
        | bit(wide, 0x04);
    a[15] = checksum(&a[..15]);
    b[15] = checksum(&b[..15]);
    Some((a, b))
}

/// Screen frames for the dot-matrix display: screen 0x01 (track) with three empty lines. The mod's
/// own screen covers it; this only replaces the native idle "NO DISC". `seq` must change whenever
/// the screen changes (the panel acks it in its frame 0x02 byte 9).
pub fn blank_screen(seq: u8) -> [F; 3] {
    let mut f80 = [0u8; 16];
    f80[0] = 0x80;
    f80[1] = seq;
    f80[2] = 0x01;
    // native led_base frame 80 bytes 3..14
    f80[3..15].copy_from_slice(&[0x00, 0x01, 0x00, 0x10, 0x00, 0x00, 0x02, 0x00, 0x20, 0x03, 0x00, 0x00]);
    // records [id][attr x3][len][chars]: 0x25, 0x02, 0x03, all empty (15 bytes over 81 + 82)
    let stream = [0x25, 0x01, 0, 0, 0, 0x02, 0x01, 0, 0, 0, 0x03, 0x01, 0, 0, 0];
    let mut f81 = [0u8; 16];
    let mut f82 = [0u8; 16];
    f81[0] = 0x81;
    f82[0] = 0x82;
    f81[1..15].copy_from_slice(&stream[..14]);
    f82[1] = stream[14];
    [f80, f81, f82]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f01(sel: u16, tempo: u16, jog: u16, flags: u8) -> F {
        let mut f = [0u8; 16];
        f[0] = 0x01;
        f[1] = 0x01;
        f[3..5].copy_from_slice(&sel.to_be_bytes());
        f[5..7].copy_from_slice(&tempo.to_be_bytes());
        f[7..9].copy_from_slice(&0x7FC0u16.to_be_bytes());
        f[9..11].copy_from_slice(&jog.to_be_bytes());
        f[11..13].copy_from_slice(&0xFFFFu16.to_be_bytes());
        f[13] = flags;
        f[15] = checksum(&f[..15]);
        f
    }

    fn f02(b: [u8; 8]) -> F {
        let mut f = [0u8; 16];
        f[0] = 0x02;
        f[1..8].copy_from_slice(&b[1..8]);
        f[8] = 0xF0;
        f[9] = 0x06;
        f[10] = 0xFF;
        f[12] = checksum(&f[..12]);
        f[13..16].copy_from_slice(&[0xF1, 0x64, 0x76]);
        f
    }

    #[test]
    fn checksum_matches_capture() {
        let a = [0x01, 0x02, 0x58, 0x03, 0x01, 0x04, 0x32, 0x4a, 0x00, 0xae, 0, 0, 0, 0, 0, 0x8e];
        assert_eq!(checksum(&a[..15]), a[15]);
    }

    #[test]
    fn transport_is_instant_ladders_are_debounced() {
        let mut d = Decoder::new();
        assert!(d.feed(&f02([0, 0x02, 0, 0, 0, 0, 0, 0x01]), 1000)); // PLAY + BROWSE
        let r = d.report();
        assert_eq!(r[0], 0x80);
        assert_eq!(r[5], 0x00); // BROWSE not yet
        d.settle(1059);
        assert_eq!(d.report()[5], 0x00);
        d.settle(1060);
        assert_eq!(d.report()[5], 0x01);
        // a 30 ms ghost (TAG LIST) before INFO is dropped
        d.feed(&f02([0, 0, 0, 0, 0, 0, 0, 0x02]), 2000);
        d.feed(&f02([0, 0, 0, 0, 0, 0, 0, 0x04]), 2030);
        d.settle(2080);
        assert_eq!(d.report()[5], 0x01);
        d.settle(2090);
        assert_eq!(d.report()[5], 0x04);
        assert!(!d.feed(&{ let mut f = f02([0; 8]); f[12] ^= 1; f }, 3000));
    }

    #[test]
    fn every_key_lands_somewhere() {
        for &(fb, fm, hb, hm) in KEYS.iter() {
            let mut b = [0u8; 8];
            b[fb] = fm;
            let mut d = Decoder::new();
            d.feed(&f02(b), 0);
            d.settle(100);
            let r = d.report();
            assert_eq!(r[hb] & hm, hm, "frame byte {fb} mask {fm:#x}");
        }
    }

    #[test]
    fn jog_selector_tempo() {
        let mut d = Decoder::new();
        d.feed(&f01(0, 0x7FC0, 0xFFFE, 0), 0);
        let j0 = u16::from_le_bytes([d.report()[14], d.report()[15]]);
        d.feed(&f01(1, 0x7FC0, 0x0001, 0xC0 | 0x20 | 0x01), 1); // +3 counts across the wrap, CW, touch, push
        let r = d.report();
        assert_eq!(u16::from_le_bytes([r[14], r[15]]), j0.wrapping_add(9));
        assert_eq!(r[7] & 0xE0, 0xE0);
        assert_eq!(r[3] & 0x20, 0x20);
        assert_eq!(r[10], 12);
        assert_eq!(i16::from_le_bytes([r[12], r[13]]), 0);
        d.feed(&f01(0xFFFF, 0x0000, 1, 0), 2); // selector -2, fader fully towards the player
        let r = d.report();
        assert_eq!(r[10], 10);
        assert_eq!(i16::from_le_bytes([r[12], r[13]]), 1000);
        d.feed(&f01(0xFFFF, 0xFFFF, 1, 0), 3);
        assert_eq!(i16::from_le_bytes([d.report()[12], d.report()[13]]), -1000);
        d.feed(&f01(0xFFFF, (0x7FC0 - 48 - (0x7FC0 - 48) / 2) as u16, 1, 0), 4);
        assert_eq!(i16::from_le_bytes([d.report()[12], d.report()[13]]), 500);
        d.feed(&f01(0xFFFF, (0x7FC0 - 48 - (0x7FC0 - 48) / 2 + 10) as u16, 1, 0), 5); // jitter: no change
        assert_eq!(i16::from_le_bytes([d.report()[12], d.report()[13]]), 500);
    }

    fn out() -> [u8; OUT_LEN] {
        let mut o = [0u8; OUT_LEN];
        o[0] = 0x01;
        o
    }

    #[test]
    fn lamps_and_fields() {
        let mut o = out();
        o[4] = 0x80 | 0x40 | 0x08; // PLAY CUE RELOOP
        o[5] = 0x80 | 0x20; // MT, +-10
        o[6] = 0x01 | 0x10 | 0x80; // AUTO LOOP 1, REV, VINYL
        o[7] = 0x01; // touch
        o[8] = 0x01 | 0x40; // BROWSE MENU
        o[11] = 0xF1;
        o[12] = 3;
        o[13] = 46;
        o[14] = 12;
        o[18] = 7;
        o[20..22].copy_from_slice(&128u16.to_le_bytes());
        o[22..24].copy_from_slice(&(-250i16).to_le_bytes());
        let (a, b) = encode(&o).unwrap();
        assert_eq!(&a[4..12], &[7, 3, 46, 9, 0, 128, 0x00, 250]);
        assert_eq!(a[2], 0x40 | 0x20 | 0x18);
        assert_eq!(b[5], 0x08 | 0x10);
        assert_eq!(b[6], 0x01 | 0x08 | 0x10 | 0x20);
        assert_eq!(b[7], 0x01 | 0x10 | 0x80);
        assert_eq!(b[11], 0x02 | 0x04 | 0x80);
        assert_eq!(b[14], 0x0A | 0x40);
        assert_eq!(b[13], 0x0E | 0x20);
        // ring from time: 3:46.12 = 22612 hs, % 180 = 112 -> 112*135/180 + 1 = 85
        assert_eq!(a[3], 85);
        assert_eq!(checksum(&a[..15]), a[15]);
        assert_eq!(checksum(&b[..15]), b[15]);
    }

    #[test]
    fn remain_time_is_inverted_back() {
        let mut o = out();
        o[4] = 0x02;
        o[11] = 0x01;
        o[12] = 0x77 - 4;
        o[13] = 0x8D - 50;
        o[14] = 0x79 - 40;
        let (a, b) = encode(&o).unwrap();
        assert_eq!(&a[5..8], &[4, 50, 30]);
        assert_eq!(b[13] & 0x01, 0x01);
    }

    #[test]
    fn extension_bytes() {
        let mut o = out();
        o[24] = 40;
        o[25] = 100;
        o[26] = 37;
        o[27] = 12;
        o[28] = 0x81;
        o[29] = 0x01 | 0x08;
        let (a, b) = encode(&o).unwrap();
        assert_eq!((a[1], b[1], a[3], b[3]), (40, 40, 100, 100));
        assert_eq!((b[4], b[10], b[9]), (37, 12, 0x04));
        assert_eq!(b[5] & 0x02, 0x02);
        assert_eq!(b[6] & 0x40, 0x40);
        assert_eq!(b[12] & 0x08, 0x08);
        // no cue, ring off, ext not valid -> ring off, mark 0, bar 0
        let mut o = out();
        o[24] = 0xFF;
        o[25] = 0x88;
        o[27] = 0xFF;
        let (a, b) = encode(&o).unwrap();
        assert_eq!((a[1], a[3], b[10], b[13] & 0x20), (0, 0x88, 0, 0));
        // stock MAIN ignores 24..35 = 0xFF; so do we without the valid flag
        let mut o = out();
        for x in &mut o[24..36] {
            *x = 0xFF;
        }
        o[28] = 0x7F;
        let (a, b) = encode(&o).unwrap();
        assert_eq!((a[1], a[3], b[4], b[9], b[10]), (0, 0x88, 0, 0, 0));
        assert!(encode(&{ let mut o = out(); o[0] = 2; o }).is_none());
    }

    #[test]
    fn screen_frames() {
        let [f80, f81, f82] = blank_screen(5);
        assert_eq!((f80[0], f80[1], f80[2]), (0x80, 5, 1));
        assert_eq!(&f81[1..6], &[0x25, 1, 0, 0, 0]);
        assert_eq!((f82[0], f82[1]), (0x82, 0));
    }
}
