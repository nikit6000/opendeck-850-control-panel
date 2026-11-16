//! Minimal USB HID class with the CDJ-850 PC-mode report descriptor: vendor page 0xFFA0, 20-byte input,
//! 36-byte output, no report IDs, interrupt IN + OUT at 1 ms. Output also accepted via SET_REPORT.

use usb_device::class_prelude::*;
use usb_device::control::{Recipient, Request, RequestType};
use usb_device::Result;

use crate::panel::{IN_LEN, OUT_LEN};

/// The stock CDJ-850's HID report descriptor (docs/USB-HID.md), byte for byte.
const REPORT_DESC: &[u8] = &[
    0x06, 0xA0, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x09, 0x02, 0xA1, 0x00, 0x06, 0xA1, 0xFF, 0x09, 0x03,
    0x09, 0x04, 0x15, 0x80, 0x25, 0x7F, 0x35, 0x00, 0x45, 0xFF, 0x75, 0x08, 0x95, 0x14, 0x81, 0x02,
    0x09, 0x05, 0x09, 0x06, 0x15, 0x80, 0x25, 0x7F, 0x35, 0x00, 0x45, 0xFF, 0x75, 0x08, 0x95, 0x24,
    0x91, 0x02, 0xC0, 0xC0,
];

const DT_HID: u8 = 0x21;
const DT_REPORT: u8 = 0x22;
const GET_REPORT: u8 = 0x01;
const GET_IDLE: u8 = 0x02;
const GET_PROTOCOL: u8 = 0x03;
const SET_REPORT: u8 = 0x09;
const SET_IDLE: u8 = 0x0A;
const SET_PROTOCOL: u8 = 0x0B;

pub struct Hid<'a, B: UsbBus> {
    iface: InterfaceNumber,
    ep_in: EndpointIn<'a, B>,
    ep_out: EndpointOut<'a, B>,
    input: [u8; IN_LEN],
    output: Option<[u8; OUT_LEN]>,
    idle: u8,
}

impl<'a, B: UsbBus> Hid<'a, B> {
    pub fn new(alloc: &'a UsbBusAllocator<B>) -> Self {
        Hid {
            iface: alloc.interface(),
            ep_in: alloc.interrupt(32, 1),
            ep_out: alloc.interrupt(64, 1),
            input: [0; IN_LEN],
            output: None,
            idle: 0,
        }
    }

    /// Send an input report; false if the IN endpoint is still busy (try again later).
    pub fn send(&mut self, r: &[u8; IN_LEN]) -> bool {
        match self.ep_in.write(r) {
            Ok(_) => {
                self.input = *r;
                true
            }
            Err(_) => false,
        }
    }

    /// The latest output report from the host, once.
    pub fn take_output(&mut self) -> Option<[u8; OUT_LEN]> {
        self.output.take()
    }

    fn set_output(&mut self, d: &[u8]) {
        if d.len() >= OUT_LEN {
            let mut o = [0u8; OUT_LEN];
            o.copy_from_slice(&d[..OUT_LEN]);
            self.output = Some(o);
        }
    }
}

impl<B: UsbBus> UsbClass<B> for Hid<'_, B> {
    fn get_configuration_descriptors(&self, w: &mut DescriptorWriter) -> Result<()> {
        w.interface(self.iface, 0x03, 0x00, 0x00)?;
        let n = REPORT_DESC.len() as u16;
        w.write(DT_HID, &[0x11, 0x01, 0x00, 0x01, DT_REPORT, n as u8, (n >> 8) as u8])?;
        w.endpoint(&self.ep_in)?;
        w.endpoint(&self.ep_out)?;
        Ok(())
    }

    fn reset(&mut self) {
        self.output = None;
        self.idle = 0;
    }

    fn endpoint_out(&mut self, addr: EndpointAddress) {
        if addr != self.ep_out.address() {
            return;
        }
        let mut buf = [0u8; 64];
        if let Ok(n) = self.ep_out.read(&mut buf) {
            self.set_output(&buf[..n]);
        }
    }

    fn control_in(&mut self, xfer: ControlIn<B>) {
        let req = *xfer.request();
        if req.recipient != Recipient::Interface || req.index != u8::from(self.iface) as u16 {
            return;
        }
        match (req.request_type, req.request) {
            (RequestType::Standard, Request::GET_DESCRIPTOR) => match (req.value >> 8) as u8 {
                DT_REPORT => {
                    xfer.accept_with_static(REPORT_DESC).ok();
                }
                DT_HID => {
                    let n = REPORT_DESC.len() as u16;
                    let d = [9, DT_HID, 0x11, 0x01, 0x00, 0x01, DT_REPORT, n as u8, (n >> 8) as u8];
                    xfer.accept_with(&d).ok();
                }
                _ => {
                    xfer.reject().ok();
                }
            },
            (RequestType::Class, GET_REPORT) => {
                let r = self.input;
                xfer.accept_with(&r).ok();
            }
            (RequestType::Class, GET_IDLE) => {
                let i = [self.idle];
                xfer.accept_with(&i).ok();
            }
            (RequestType::Class, GET_PROTOCOL) => {
                xfer.accept_with(&[1]).ok();
            }
            _ => {}
        }
    }

    fn control_out(&mut self, xfer: ControlOut<B>) {
        let req = *xfer.request();
        if req.request_type != RequestType::Class
            || req.recipient != Recipient::Interface
            || req.index != u8::from(self.iface) as u16
        {
            return;
        }
        match req.request {
            SET_REPORT => {
                let mut d = [0u8; OUT_LEN];
                let n = xfer.data().len().min(OUT_LEN);
                d[..n].copy_from_slice(&xfer.data()[..n]);
                if n == OUT_LEN {
                    self.set_output(&d);
                }
                xfer.accept().ok();
            }
            SET_IDLE => {
                self.idle = (req.value >> 8) as u8;
                xfer.accept().ok();
            }
            SET_PROTOCOL => {
                xfer.accept().ok();
            }
            _ => {
                xfer.reject().ok();
            }
        }
    }
}
