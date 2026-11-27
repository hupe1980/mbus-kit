//! Arbitrary bytes through the whole wireless chain with a fixed key: link layer (every
//! format), ELL, AFL, TPL, decryption, MAC and CRC checks, records and Display
//! (002-oms-security/SR-004). The first byte picks the CI for the application-only path.
#![no_main]

use std::fmt::Write;

use libfuzzer_sys::fuzz_target;
use mbus_kit::oms::Key;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body};

const KEY: Key = Key::new([0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x11]);

fn drain(app: &Application<'_>, out: &mut String) {
    if let Body::Records(r) = &app.body {
        for rec in r.clone().flatten() {
            let _ = write!(out, "{rec}");
        }
    }
    out.clear();
}

fuzz_target!(|data: &[u8]| {
    let mut out = String::new();
    for format in [Format::A, Format::B, Format::Stripped] {
        let mut buf = [0u8; 600];
        if let Ok(Some((t, _))) = wireless::open(data, format, &KEY, &mut buf) {
            drain(&t.app, &mut out);
        }
    }
    // The application layer alone, link address fixed, every decryption path.
    let Some((&ci, rest)) = data.split_first() else { return };
    let link = mbus_kit::Address::from_link_bytes([0x93, 0x15, 0x78, 0x56, 0x34, 0x12, 0x33, 0x03]);
    if let Ok(app) = Application::parse_with_link(ci, rest, Some(link)) {
        drain(&app, &mut out);
        if let Body::Encrypted(e) = &app.body {
            let mut plain = [0u8; 300];
            if let Ok(inner) = e.decrypt(&KEY, &mut plain) {
                drain(&inner, &mut out);
            }
        }
    }
});
