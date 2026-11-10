//! CDJ-850 panel-bus adapter for STM32F401 ("Black Pill", 25 MHz HSE): plays the MAIN board.
//!
//! Bus (PROTOCOL.md): SPI mode 3, LSB first, 16-byte full-duplex frames, ~1.3 MHz clock.
//! **MAIN is the clock master** (captures/disc_check: the panel raises PNL_XBUSY 4 µs after a frame
//! and MAIN starts the next clock burst 64 µs after PNL_XBUSY falls; with MAIN unplugged the bus
//! never starts and the panel shows E-8709). MAIN sends TSO, the panel answers on TSI.
//!
//! Pins (each through 1 kΩ to CN2007):
//! - PA5 = TSCK, PA7 = TSO: SPI1. MASTER mode: SPI1 is the master and drives both (clock /64 =
//!   1.3125 MHz, TX by DMA2 S3 ch3). LISTEN mode: SPI1 is an RX-only slave (DMA2 S0 ch3) = sniffer.
//! - PB13 ← TSCK, PB15 ← TSI: SPI2 RX-only slave (DMA1 S3 ch0) in both modes — the panel's answer.
//! - PA15 ← TSCK: TIM2 CH1, slave reset mode; CC2 fires after GAP_US of clock silence = frame end.
//!   The ISR hands the frame to USB, resets the slave SPIs (bit realignment), loads the next answer.
//! - PB0 ← PNL_XBUSY (MASTER waits while it is high).
//! - PB1 = MAIN_XBUSY: MASTER / SLAVE drive it high ("busy") until BOOT_DELAY_US after PWR_ON,
//!   then low ("ready", as in every native capture); LISTEN: input.
//! - PA1 ← PWR_ON (CN2007 pin 9): MASTER starts clocking BOOT_DELAY_US after it goes high.
//! - PB14 is not used (input, Hi-Z). PC13 LED blinks while frames flow.
//!
//! MASTER schedule (TIM3 tick every 10 µs): start a frame when ≥ MIN_GAP_US passed since the last one
//! ended and PNL_XBUSY has been low for ≥ XBUSY_SETTLE_US. Frames, like the native MAIN: slots
//! `02, x, x, 01, x, x` repeating, x walking through the "others" list (default = idle screen 80, 81,
//! 82, 60). 0x60 is the heartbeat, generated here (byte 1 = +1 every 20 ms); checksums are computed
//! here. Until the host sends anything, the frames are the native MAIN's idle frames (disc_check).
//!
//! USB CDC, device → host, 44-byte record per bus frame (little-endian):
//!   A5 5A | seq u16 | t_us u32 | n_tso u8 | n_tsi u8 | flags u8 | dropped u8 | TSO[16] | TSI[16]
//!   flags: bit0 PNL_XBUSY, bit1 MAIN_XBUSY, bit2 MASTER mode, bit3 PWR_ON, bit4 SLAVE. In MASTER, TSO = what we sent.
//! Host → device commands: A5 5A | cmd u8 | len u8 | payload[len]
//!   0x01 len 32: frames 0x01 and 0x02 (16 B each; byte 15 ignored)
//!   0x02 len 16·k (k = 1..15): "others" list in send order; 0x60 is added automatically if missing
//!   0x03 len 1: mode, 0 = LISTEN (sniffer, native MAIN connected), 1 = MASTER (we clock the bus),
//!               2 = SLAVE (the panel clocks, we answer on PB14 → TSO and hold MAIN_XBUSY low)
#![no_std]
#![no_main]

use core::cell::RefCell;

use cortex_m::peripheral::NVIC;
use cortex_m_rt::entry;
use critical_section::Mutex;
use heapless::{Deque, Vec};
use panic_halt as _;
use stm32f4xx_hal::{
    otg_fs::{UsbBus, USB},
    pac::{self, interrupt, Interrupt},
    prelude::*,
    rcc::Config,
};
use usb_device::prelude::*;
use usbd_serial::SerialPort;

const REC: usize = 44;
const GAP_US: u32 = 40; // bytes inside a frame are back to back, frames ≥ 600 µs apart
const TIMCLK_MHZ: u32 = 84;
const BUF: usize = 32;
const MAX_OTHERS: usize = 16;
const MIN_GAP_US: u32 = 610; // native MAIN: ~612 µs from frame end to next start
const XBUSY_SETTLE_US: u32 = 60; // native MAIN: ~64 µs from PNL_XBUSY fall to clock start
const XFER_TIMEOUT_US: u32 = 2000;
/// MASTER starts clocking this long after PWR_ON (PA1) goes high — the native MAIN boots ~2.8 s
/// before its first frame; clocking a panel that is still starting misaligns its SPI for good.
const BOOT_DELAY_US: u32 = 2_800_000;

static mut RX_TSO: [u8; BUF] = [0; BUF];
static mut RX_TSI: [u8; BUF] = [0; BUF];
static mut TX: [u8; 16] = [0xFF; 16];
static mut EP_MEMORY: [u32; 1024] = [0; 1024];

type F = [u8; 16];

/// Idle frames of the native MAIN with no media (captures/disc_check), checksums recomputed.
const IDLE_01: F = [0x01, 0x00, 0x58, 0, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
const IDLE_02: F = [0x02, 0x00, 0x58, 0, 0, 0x60, 0x22, 0x1F, 0, 0, 0, 0x04, 0x51, 0x2F, 0x4A, 0];
const IDLE_80: F = [0x80, 0x01, 0x01, 0, 0, 0, 0, 0, 0, 0x02, 0, 0, 0x02, 0, 0, 0];
const IDLE_81: F = [0x81, 0x11, 0, 0, 0, 0, 0x12, 0, 0, 0, 0, 0x13, 0, 0, 0, 0];
const IDLE_82: F = frame(0x82);
const HEARTBEAT: F = frame(0x60);

const fn frame(t: u8) -> F {
    let mut f = [0u8; 16];
    f[0] = t;
    f
}

struct State {
    q: Deque<[u8; REC], 128>,
    seq: u16,
    dropped: u8,
    frames: u32,
    master: bool,
    slave_tx: bool,
    f01: F,
    f02: F,
    others: Vec<F, MAX_OTHERS>,
    slot: u8,
    other_i: usize,
    // MASTER scheduling
    in_xfer: bool,
    xfer_start: u32,
    last_end: u32,
    xbusy_low_since: Option<u32>,
    pwr_on_since: Option<u32>,
}

static STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    q: Deque::new(),
    seq: 0,
    dropped: 0,
    frames: 0,
    master: false,
    slave_tx: false,
    f01: IDLE_01,
    f02: IDLE_02,
    others: Vec::new(),
    slot: 0,
    other_i: 0,
    in_xfer: false,
    xfer_start: 0,
    last_end: 0,
    xbusy_low_since: None,
    pwr_on_since: None,
}));

/// 8-bit one's-complement sum (end-around carry), as MAIN fw 0x0C0F9EE8.
fn checksum(b: &[u8]) -> u8 {
    let mut s: u32 = 0;
    for &x in b {
        s += x as u32;
        if s > 0xFF {
            s = (s & 0xFF) + 1;
        }
    }
    s as u8
}

impl State {
    /// Next frame to send (checksum filled in).
    fn next_frame(&mut self, now_us: u32) -> F {
        let s = self.slot;
        self.slot = (self.slot + 1) % 6;
        let mut f = match s {
            0 => self.f02,
            3 => self.f01,
            _ => {
                if self.others.is_empty() {
                    HEARTBEAT
                } else {
                    let f = self.others[self.other_i % self.others.len()];
                    self.other_i = (self.other_i + 1) % self.others.len();
                    f
                }
            }
        };
        if f[0] == 0x60 {
            f = HEARTBEAT;
            f[1] = (now_us / 20_000) as u8;
        }
        f[15] = checksum(&f[..15]);
        f
    }

    fn set_others(&mut self, frames: &[u8]) {
        self.others.clear();
        let mut has_hb = false;
        for c in frames.chunks_exact(16) {
            let mut f = [0u8; 16];
            f.copy_from_slice(c);
            has_hb |= f[0] == 0x60;
            if self.others.push(f).is_err() {
                break;
            }
        }
        if !has_hb {
            let _ = self.others.push(HEARTBEAT);
        }
        self.other_i = 0;
    }
}

fn now_us() -> u32 {
    unsafe { (*pac::TIM5::ptr()).cnt().read().bits() }
}

fn xbusy() -> bool {
    unsafe { (*pac::GPIOB::ptr()).idr().read().bits() & 1 != 0 }
}

/// PA1 ← PWR_ON (CN2007 pin 9), pulled down: low = player off / starting.
fn pwr_on() -> bool {
    unsafe { (*pac::GPIOA::ptr()).idr().read().bits() & (1 << 1) != 0 }
}

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let mut rcc = dp.RCC.freeze(
        Config::hse(25.MHz()).sysclk(84.MHz()).pclk1(42.MHz()).pclk2(84.MHz()).require_pll48clk(),
    );
    let gpioa = dp.GPIOA.split(&mut rcc);
    let gpiob = dp.GPIOB.split(&mut rcc);
    let gpioc = dp.GPIOC.split(&mut rcc);

    let _tsck1 = gpioa.pa5.into_alternate::<5>().speed(stm32f4xx_hal::gpio::Speed::High);
    let _tso = gpioa.pa7.into_alternate::<5>().speed(stm32f4xx_hal::gpio::Speed::High);
    let _tsck2 = gpiob.pb13.into_alternate::<5>();
    let _tsi = gpiob.pb15.into_alternate::<5>();
    let _tsck3 = gpioa.pa15.into_alternate::<1>();
    let _pnl_busy = gpiob.pb0.into_pull_down_input(); // high = panel busy; unwired → never busy
    let _main_busy = gpiob.pb1.into_floating_input();
    let _pb14 = gpiob.pb14.into_floating_input();
    let _pwr_on = gpioa.pa1.into_pull_down_input();
    let mut led = gpioc.pc13.into_push_pull_output();
    led.set_high();

    let r = unsafe { &*pac::RCC::ptr() };
    r.ahb1enr().modify(|v, w| unsafe { w.bits(v.bits() | (1 << 21) | (1 << 22)) }); // DMA1, DMA2
    r.apb1enr()
        .modify(|v, w| unsafe { w.bits(v.bits() | (1 << 14) | (1 << 0) | (1 << 1) | (1 << 3)) }); // SPI2, TIM2, TIM3, TIM5
    r.apb2enr().modify(|v, w| unsafe { w.bits(v.bits() | (1 << 12)) }); // SPI1

    critical_section::with(|cs| {
        let mut st = STATE.borrow_ref_mut(cs);
        let mut idle = [0u8; 48];
        idle[..16].copy_from_slice(&IDLE_80);
        idle[16..32].copy_from_slice(&IDLE_81);
        idle[32..].copy_from_slice(&IDLE_82);
        st.set_others(&idle);
    });

    // TIM5: free-running 1 MHz timestamp
    let t5 = unsafe { &*pac::TIM5::ptr() };
    t5.psc().write(|w| unsafe { w.bits(TIMCLK_MHZ - 1) });
    t5.arr().write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    t5.egr().write(|w| unsafe { w.bits(1) });
    t5.cr1().write(|w| unsafe { w.bits(1) });

    set_mode(0);

    // TIM2: gap detector (frame end)
    let t2 = unsafe { &*pac::TIM2::ptr() };
    t2.psc().write(|w| unsafe { w.bits(0) });
    t2.arr().write(|w| unsafe { w.bits(0xFFFF_FFFF) });
    t2.ccmr1_input().write(|w| unsafe { w.bits(0b01 | (0b0011 << 4)) }); // CC1S = TI1, IC1F N=8
    t2.ccer().write(|w| unsafe { w.bits(0) }); // rising edge
    t2.ccr2().write(|w| unsafe { w.bits(GAP_US * TIMCLK_MHZ) });
    t2.smcr().write(|w| unsafe { w.bits(0b100 | (0b101 << 4)) }); // SMS = reset, TS = TI1FP1
    t2.egr().write(|w| unsafe { w.bits(1) });
    t2.sr().write(|w| unsafe { w.bits(0) });
    t2.dier().write(|w| unsafe { w.bits(1 << 2) }); // CC2IE
    t2.cr1().write(|w| unsafe { w.bits(1) });

    // TIM3: 10 µs scheduler tick (MASTER mode)
    let t3 = unsafe { &*pac::TIM3::ptr() };
    t3.psc().write(|w| unsafe { w.bits(0) });
    t3.arr().write(|w| unsafe { w.bits(10 * TIMCLK_MHZ - 1) });
    t3.egr().write(|w| unsafe { w.bits(1) });
    t3.sr().write(|w| unsafe { w.bits(0) });
    t3.dier().write(|w| unsafe { w.bits(1) }); // UIE
    t3.cr1().write(|w| unsafe { w.bits(1) });

    unsafe {
        // frame end must not be delayed by the scheduler tick
        let mut cp = cortex_m::Peripherals::steal();
        cp.NVIC.set_priority(Interrupt::TIM2, 0x10);
        cp.NVIC.set_priority(Interrupt::TIM3, 0x20);
        NVIC::unmask(Interrupt::TIM2);
        NVIC::unmask(Interrupt::TIM3);
    }

    // USB CDC
    let usb = USB::new(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        (gpioa.pa11, gpioa.pa12),
        &rcc.clocks,
    );
    let bus = UsbBus::new(usb, unsafe { &mut *core::ptr::addr_of_mut!(EP_MEMORY) });
    let mut serial = SerialPort::new(&bus);
    let mut dev = UsbDeviceBuilder::new(&bus, UsbVidPid(0x1209, 0x0850))
        .strings(&[StringDescriptors::default()
            .manufacturer("cdj850")
            .product("CDJ-850 panel bus adapter")
            .serial_number("0003")])
        .unwrap()
        .device_class(usbd_serial::USB_CLASS_CDC)
        .build();

    let mut out = [0u8; REC];
    let mut pos = REC;
    let mut last_blink = 0u32;
    let mut cmd: Vec<u8, 300> = Vec::new();
    loop {
        dev.poll(&mut [&mut serial]);

        // host → device
        let mut rx = [0u8; 64];
        if let Ok(n) = serial.read(&mut rx) {
            for &b in &rx[..n] {
                if cmd.push(b).is_err() {
                    cmd.clear();
                }
                if cmd.len() == 1 && cmd[0] != 0xA5 || cmd.len() == 2 && cmd[1] != 0x5A {
                    cmd.clear();
                    if b == 0xA5 {
                        let _ = cmd.push(b);
                    }
                    continue;
                }
                if cmd.len() >= 4 && cmd.len() == 4 + cmd[3] as usize {
                    handle(cmd[2], &cmd[4..]);
                    cmd.clear();
                }
            }
        }

        // device → host
        if pos == REC {
            if let Some(rec) = critical_section::with(|cs| STATE.borrow_ref_mut(cs).q.pop_front()) {
                out = rec;
                pos = 0;
            }
        }
        if pos < REC {
            match serial.write(&out[pos..]) {
                Ok(n) => pos += n,
                Err(UsbError::WouldBlock) => {}
                Err(_) => pos = REC,
            }
        }
        let frames = critical_section::with(|cs| STATE.borrow_ref(cs).frames);
        if frames.wrapping_sub(last_blink) >= 1024 {
            last_blink = frames;
            led.toggle();
        }
    }
}

fn handle(cmd: u8, p: &[u8]) {
    match (cmd, p.len()) {
        (0x01, 32) => critical_section::with(|cs| {
            let mut st = STATE.borrow_ref_mut(cs);
            st.f01.copy_from_slice(&p[..16]);
            st.f02.copy_from_slice(&p[16..]);
        }),
        (0x02, n) if n >= 16 && n % 16 == 0 => {
            critical_section::with(|cs| STATE.borrow_ref_mut(cs).set_others(p))
        }
        (0x03, 1) => {
            // re-sending the current mode must not restart the MAIN_XBUSY handshake
            let cur = critical_section::with(|cs| {
                let st = STATE.borrow_ref(cs);
                if st.master { 1 } else if st.slave_tx { 2 } else { 0 }
            });
            if p[0] != cur {
                set_mode(p[0]);
            }
        }
        _ => {}
    }
}

const SPI_CPHA_CPOL_LSB_SSM: u16 = (1 << 0) | (1 << 1) | (1 << 7) | (1 << 9);

/// Modes: 0 LISTEN (both SPIs RX-only slaves), 1 MASTER (SPI1 drives TSCK + TSO),
/// 2 SLAVE (the panel clocks; SPI2 answers on PB14 → TSO, as the native MAIN's SCIF does).
fn set_mode(mode: u8) {
    let master = mode == 1;
    let slave_tx = mode == 2;
    critical_section::with(|cs| {
        let mut st = STATE.borrow_ref_mut(cs);
        st.master = master;
        st.slave_tx = slave_tx;
        st.slot = 0;
        if slave_tx {
            let f = st.next_frame(now_us());
            unsafe { (*core::ptr::addr_of_mut!(TX)).copy_from_slice(&f) };
        }
        st.in_xfer = false;
        st.last_end = now_us();
        st.xbusy_low_since = None;
        let r = unsafe { &*pac::RCC::ptr() };
        let s1 = unsafe { &*pac::SPI1::ptr() };
        let d2 = unsafe { &*pac::DMA2::ptr() };
        for s in [d2.st(0), d2.st(3)] {
            s.cr().modify(|v, w| unsafe { w.bits(v.bits() & !1) });
            while s.cr().read().bits() & 1 != 0 {}
        }
        r.apb2rstr().modify(|v, w| unsafe { w.bits(v.bits() | (1 << 12)) });
        r.apb2rstr().modify(|v, w| unsafe { w.bits(v.bits() & !(1 << 12)) });
        // PB1 = MAIN_XBUSY: MASTER / SLAVE drive it low ("MAIN ready", as in every native
        // capture); LISTEN leaves it an input. PB14 = SPI2 MISO (AF5) only in SLAVE.
        let gb = unsafe { &*pac::GPIOB::ptr() };
        gb.bsrr().write(|w| unsafe { w.bits(1 << 1) }); // ODR1 = 1 (busy) until TIM3 says ready
        st.pwr_on_since = None;
        gb.afrh().modify(|v, w| unsafe { w.bits((v.bits() & !(0xF << 24)) | (5 << 24)) });
        gb.moder().modify(|v, w| unsafe {
            let m = (v.bits() & !(0b11 << 2) & !(0b11 << 28))
                | if mode != 0 { 0b01 << 2 } else { 0 }
                | if slave_tx { 0b10 << 28 } else { 0 };
            w.bits(m)
        });
        if master {
            // master, fPCLK2/64 = 1.3125 MHz, SSI = 1 (needed with SSM in master mode)
            let cr1 = SPI_CPHA_CPOL_LSB_SSM | (1 << 2) | (0b101 << 3) | (1 << 8);
            s1.cr1().write(|w| unsafe { w.bits(cr1) });
            s1.cr1().write(|w| unsafe { w.bits(cr1 | (1 << 6)) }); // SPE: SCK idles high
        }
        rearm(master, slave_tx);
    });
}

/// Reset the slave SPIs and restart their RX DMA (called at mode switch and after every frame).
fn rearm(master: bool, slave_tx: bool) {
    let r = unsafe { &*pac::RCC::ptr() };
    let d1 = unsafe { &*pac::DMA1::ptr() };
    let d2 = unsafe { &*pac::DMA2::ptr() };
    let s1 = unsafe { &*pac::SPI1::ptr() };
    let s2 = unsafe { &*pac::SPI2::ptr() };

    let mut streams: Vec<_, 4> = Vec::new();
    let _ = streams.push(d1.st(3));
    let _ = streams.push(d1.st(4));
    if !master {
        let _ = streams.push(d2.st(0));
    }
    let _ = streams.push(d2.st(3));
    for s in streams.iter() {
        s.cr().modify(|v, w| unsafe { w.bits(v.bits() & !1) });
        while s.cr().read().bits() & 1 != 0 {}
    }
    d2.lifcr().write(|w| unsafe { w.bits(0x0F7D_0F7D) });
    d1.lifcr().write(|w| unsafe { w.bits(0x0F7D_0F7D) });
    d1.hifcr().write(|w| unsafe { w.bits(0x0F7D_0F7D) });

    r.apb1rstr().modify(|v, w| unsafe { w.bits(v.bits() | (1 << 14)) });
    r.apb1rstr().modify(|v, w| unsafe { w.bits(v.bits() & !(1 << 14)) });
    if !master {
        r.apb2rstr().modify(|v, w| unsafe { w.bits(v.bits() | (1 << 12)) });
        r.apb2rstr().modify(|v, w| unsafe { w.bits(v.bits() & !(1 << 12)) });
    }

    let rx_cfg = |ch: u32| (ch << 25) | (1 << 10) | 1; // periph → mem, MINC, EN
    let rx = d1.st(3);
    rx.par().write(|w| unsafe { w.bits(s2.dr().as_ptr() as u32) });
    rx.m0ar().write(|w| unsafe { w.bits(core::ptr::addr_of!(RX_TSI) as u32) });
    rx.ndtr().write(|w| unsafe { w.bits(BUF as u32) });
    rx.cr().write(|w| unsafe { w.bits(rx_cfg(0)) });
    let slave_rx = SPI_CPHA_CPOL_LSB_SSM | (1 << 10); // slave, RX only, NSS selected
    if slave_tx {
        // full duplex slave: answer from TX on MISO (PB14) — DMA1 S4 ch0, mem → periph
        let tx = d1.st(4);
        tx.par().write(|w| unsafe { w.bits(s2.dr().as_ptr() as u32) });
        tx.m0ar().write(|w| unsafe { w.bits(core::ptr::addr_of!(TX) as u32) });
        tx.ndtr().write(|w| unsafe { w.bits(16) });
        tx.cr().write(|w| unsafe { w.bits((1 << 10) | (0b01 << 6) | 1) });
        let fd = SPI_CPHA_CPOL_LSB_SSM;
        s2.cr1().write(|w| unsafe { w.bits(fd) });
        s2.cr2().write(|w| unsafe { w.bits(0b11) }); // RXDMAEN | TXDMAEN: first byte loads now
        s2.cr1().write(|w| unsafe { w.bits(fd | (1 << 6)) });
    } else {
        s2.cr1().write(|w| unsafe { w.bits(slave_rx) });
        s2.cr2().write(|w| unsafe { w.bits(1) }); // RXDMAEN
        s2.cr1().write(|w| unsafe { w.bits(slave_rx | (1 << 6)) });
    }

    if !master {
        let rx = d2.st(0);
        rx.par().write(|w| unsafe { w.bits(s1.dr().as_ptr() as u32) });
        rx.m0ar().write(|w| unsafe { w.bits(core::ptr::addr_of!(RX_TSO) as u32) });
        rx.ndtr().write(|w| unsafe { w.bits(BUF as u32) });
        rx.cr().write(|w| unsafe { w.bits(rx_cfg(3)) });
        s1.cr1().write(|w| unsafe { w.bits(slave_rx) });
        s1.cr2().write(|w| unsafe { w.bits(1) });
        s1.cr1().write(|w| unsafe { w.bits(slave_rx | (1 << 6)) });
    } else {
        // master: clear RXNE/OVR from the last frame (MISO is not connected)
        let _ = s1.dr().read().bits();
        let _ = s1.sr().read().bits();
        s1.cr2().write(|w| unsafe { w.bits(0) });
    }
}

/// MASTER: send TX (16 bytes) — DMA2 S3 ch3 → SPI1 DR; the master clocks while bytes come.
fn start_xfer() {
    let d2 = unsafe { &*pac::DMA2::ptr() };
    let s1 = unsafe { &*pac::SPI1::ptr() };
    let tx = d2.st(3);
    tx.par().write(|w| unsafe { w.bits(s1.dr().as_ptr() as u32) });
    tx.m0ar().write(|w| unsafe { w.bits(core::ptr::addr_of!(TX) as u32) });
    tx.ndtr().write(|w| unsafe { w.bits(16) });
    tx.cr().write(|w| unsafe { w.bits((3 << 25) | (1 << 10) | (0b01 << 6) | 1) });
    s1.cr2().write(|w| unsafe { w.bits(1 << 1) }); // TXDMAEN → starts
}

#[interrupt]
fn TIM3() {
    let t3 = unsafe { &*pac::TIM3::ptr() };
    t3.sr().write(|w| unsafe { w.bits(0) });
    let now = now_us();
    let busy = xbusy();
    critical_section::with(|cs| {
        let mut st = STATE.borrow_ref_mut(cs);
        if !st.master && !st.slave_tx {
            return;
        }
        // MAIN_XBUSY (PB1) like a booting MAIN: high ("busy", in reset) until BOOT_DELAY after
        // PWR_ON, then low ("ready").
        let gb = unsafe { &*pac::GPIOB::ptr() };
        if !pwr_on() {
            st.pwr_on_since = None;
            st.slot = 0;
            gb.bsrr().write(|w| unsafe { w.bits(1 << 1) });
            return;
        }
        let since = *st.pwr_on_since.get_or_insert(now);
        if now.wrapping_sub(since) < BOOT_DELAY_US {
            gb.bsrr().write(|w| unsafe { w.bits(1 << 1) });
            return;
        }
        gb.bsrr().write(|w| unsafe { w.bits(1 << (16 + 1)) });
        if !st.master {
            return;
        }
        if busy {
            st.xbusy_low_since = None;
        } else if st.xbusy_low_since.is_none() {
            st.xbusy_low_since = Some(now);
        }
        if st.in_xfer {
            if now.wrapping_sub(st.xfer_start) > XFER_TIMEOUT_US {
                st.in_xfer = false; // should not happen: TIM2 always sees our own clock
                st.last_end = now;
            }
            return;
        }
        let settled = st.xbusy_low_since.is_some_and(|t| now.wrapping_sub(t) >= XBUSY_SETTLE_US);
        if settled && now.wrapping_sub(st.last_end) >= MIN_GAP_US {
            let f = st.next_frame(now);
            unsafe { (*core::ptr::addr_of_mut!(TX)).copy_from_slice(&f) };
            st.in_xfer = true;
            st.xfer_start = now;
            start_xfer();
        }
    });
}

#[interrupt]
fn TIM2() {
    let t2 = unsafe { &*pac::TIM2::ptr() };
    t2.sr().write(|w| unsafe { w.bits(!(1u32 << 2)) }); // clear CC2IF
    let d1 = unsafe { &*pac::DMA1::ptr() };
    let d2 = unsafe { &*pac::DMA2::ptr() };
    let t = now_us();
    let pb = unsafe { (*pac::GPIOB::ptr()).idr().read().bits() };

    critical_section::with(|cs| {
        let mut st = STATE.borrow_ref_mut(cs);
        let master = st.master;
        let n_tsi = BUF as u32 - d1.st(3).ndtr().read().bits();
        let n_tso = if master { 16 - d2.st(3).ndtr().read().bits() } else { BUF as u32 - d2.st(0).ndtr().read().bits() };
        if n_tso == 0 && n_tsi == 0 && !st.in_xfer {
            return; // timer wrap while the bus is idle
        }
        let mut rec = [0u8; REC];
        rec[0] = 0xA5;
        rec[1] = 0x5A;
        rec[4..8].copy_from_slice(&t.to_le_bytes());
        rec[8] = n_tso as u8;
        rec[9] = n_tsi as u8;
        rec[10] = (pb & 0b11) as u8 | if master { 0b100 } else { 0 } | if st.slave_tx { 0b1_0000 } else { 0 } | if pwr_on() { 0b1000 } else { 0 };
        unsafe {
            if master {
                rec[12..28].copy_from_slice(&*core::ptr::addr_of!(TX));
            } else {
                rec[12..28].copy_from_slice(&(&*core::ptr::addr_of!(RX_TSO))[..16]);
            }
            rec[28..44].copy_from_slice(&(&*core::ptr::addr_of!(RX_TSI))[..16]);
        }
        st.frames = st.frames.wrapping_add(1);
        rec[2..4].copy_from_slice(&st.seq.to_le_bytes());
        st.seq = st.seq.wrapping_add(1);
        rec[11] = st.dropped;
        if st.q.push_back(rec).is_ok() {
            st.dropped = 0;
        } else {
            st.dropped = st.dropped.saturating_add(1);
        }
        st.in_xfer = false;
        st.last_end = t;
        let slave_tx = st.slave_tx;
        if slave_tx {
            let f = st.next_frame(t);
            unsafe { (*core::ptr::addr_of_mut!(TX)).copy_from_slice(&f) };
        }
        rearm(master, slave_tx);
    });
}
