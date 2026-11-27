//! The water meter telegram from the M-Bus documentation (m-bus.com, EN 13757-3
//! example): the one third-party test vector this crate has so far. Its length field
//! and checksum are self-consistent, so a transcription error would show.
//!
//! ```text
//! 68 1F 1F 68  08 02 72  78 56 34 12  24 40  01  07  55  00  00 00
//! 03 13 15 31 00          volume            12.565 m³
//! DA 02 3B 13 01          volume flow, max  0.113 m³/h   storage 5
//! 8B 60 04 37 18 02       energy            218370 Wh    tariff 2, subunit 1
//! 18 16
//! ```

use mbus_kit::app::ci;
use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Address, Format};
use mbus_kit::{Application, Body, Function, Header, Medium, Unit, Value};

const TELEGRAM: [u8; 37] = [
    0x68, 0x1F, 0x1F, 0x68, 0x08, 0x02, 0x72, 0x78, 0x56, 0x34, 0x12, 0x24, 0x40, 0x01, 0x07, 0x55, 0x00,
    0x00, 0x00, 0x03, 0x13, 0x15, 0x31, 0x00, 0xDA, 0x02, 0x3B, 0x13, 0x01, 0x8B, 0x60, 0x04, 0x37, 0x18,
    0x02, 0x18, 0x16,
];

fn check_application(app: &Application<'_>) {
    let Header::Long(h) = app.header else { panic!("expected a long header") };
    assert_eq!(h.address.id_number(), Some(12_345_678));
    assert_eq!(h.address.manufacturer.to_string(), "PAD");
    assert_eq!(h.address.version, 1);
    assert_eq!(h.address.medium, Medium::WATER);
    assert_eq!(h.short.access_number, 0x55);
    assert_eq!(h.short.security().mode, 0);

    let Body::Records(records) = app.body.clone() else { panic!("expected plain records") };
    let records: Vec<_> = records.collect::<Result<_, _>>().unwrap();
    assert_eq!(records.len(), 3);

    let volume = &records[0];
    assert_eq!(volume.name(), "volume");
    assert_eq!(volume.value, Value::Int(12_565));
    assert_eq!(volume.quantity().unwrap().to_string(), "12.565 m³");

    let flow = &records[1];
    assert_eq!(flow.dib.function, Function::Maximum);
    assert_eq!(flow.dib.storage, 5);
    assert_eq!(flow.quantity().unwrap().to_string(), "0.113 m³/h");

    let energy = &records[2];
    assert_eq!((energy.dib.tariff, energy.dib.subunit), (2, 1));
    let q = energy.quantity().unwrap();
    assert_eq!(q.unit, Some(Unit::Wh));
    assert_eq!(q.to_string(), "218.37 kWh");
    assert_eq!(format!("{q:#}"), "218370 Wh");
    assert_eq!(q.to_scale(3), None, "218370 Wh is not a whole number of kWh");
    assert_eq!(q.to_scale(0), Some(218_370));
}

#[test]
fn the_reference_telegram_decodes() {
    let (frame, used) = Frame::parse(&TELEGRAM).unwrap().unwrap();
    assert_eq!(used, TELEGRAM.len());
    let Frame::Long { c, a, ci, data } = frame else { panic!("expected a long frame") };
    assert_eq!((c, a, ci), (0x08, 0x02, ci::RSP_LONG));
    check_application(&Application::parse(ci, data).unwrap());
}

#[test]
fn the_reference_telegram_re_encodes_byte_for_byte() {
    let (frame, _) = Frame::parse(&TELEGRAM).unwrap().unwrap();
    let mut out = [0u8; 64];
    let n = frame.encode(&mut out).unwrap();
    assert_eq!(&out[..n], &TELEGRAM);
}

#[test]
fn the_same_payload_over_the_air() {
    let (Frame::Long { ci, data, .. }, _) = Frame::parse(&TELEGRAM).unwrap().unwrap() else { unreachable!() };
    let addr = Address {
        manufacturer: mbus_kit::Manufacturer(0x4024),
        id: [0x78, 0x56, 0x34, 0x12],
        version: 1,
        medium: Medium::WATER,
    };
    let mut wire = [0u8; 128];
    let n = wireless::encode_a(wireless::control::SND_NR, &addr, ci, data, &mut wire).unwrap();
    let mut out = [0u8; 128];
    let (frame, used) = wireless::decode(&wire[..n], Format::A, &mut out).unwrap().unwrap();
    assert_eq!(used, n);
    assert_eq!(frame.address.id_number(), Some(12_345_678));
    check_application(&Application::parse(frame.ci, frame.payload).unwrap());
}

#[test]
fn every_corruption_is_an_error_never_a_panic() {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..20_000 {
        let mut t = TELEGRAM;
        for _ in 0..=(next() % 4) {
            let i = (next() as usize) % t.len();
            t[i] = next() as u8;
        }
        let Ok(Some((Frame::Long { ci, data, .. }, _))) = Frame::parse(&t) else { continue };
        if let Ok(app) = Application::parse(ci, data) {
            if let Body::Records(r) = app.body {
                for rec in r.flatten() {
                    let _ = rec.quantity().map(|q| q.to_string());
                }
            }
        }
    }
    // The application layer on its own, checksum bypassed.
    for _ in 0..20_000 {
        let mut payload = [0u8; 30];
        for b in &mut payload {
            *b = next() as u8;
        }
        if let Ok(app) = Application::parse(ci::RSP_NONE, &payload) {
            if let Body::Records(r) = app.body {
                for rec in r.flatten() {
                    let _ = rec.quantity().map(|q| q.to_string());
                }
            }
        }
    }
}
