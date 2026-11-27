//! Arbitrary fragments through reassembly (004-compact-and-fragments/SR-005): the input,
//! split at every `FF FF`, is a sequence of AFL telegrams (CI `90` assumed) pushed into
//! one small buffer; whatever completes is decrypted with a fixed key and its records
//! rendered.
#![no_main]

use std::fmt::Write;

use libfuzzer_sys::fuzz_target;
use mbus_kit::fragment::Reassembly;
use mbus_kit::oms::Key;
use mbus_kit::{Address, Application, Body};

const KEY: Key = Key::new([0; 16]);

fuzz_target!(|data: &[u8]| {
    let link = Address::from_link_bytes([0x49, 0x6A, 0x78, 0x56, 0x34, 0x12, 0x01, 0x07]);
    let mut buf = [0u8; 128];
    let mut joined = Reassembly::new(&mut buf);
    let mut out = String::new();
    for piece in data.split(|&b| b == 0xFF).filter(|p| !p.is_empty()) {
        let Ok(app) = Application::parse_with_link(0x90, piece, Some(link)) else { continue };
        let Ok(Some(message)) = joined.push(&app) else { continue };
        let mut plain = [0u8; 128];
        let opened = match &message.body {
            Body::Encrypted(e) => e.decrypt(&KEY, &mut plain).ok(),
            _ => Some(message.clone()),
        };
        if let Some(Body::Records(r)) = opened.map(|a| a.body) {
            for rec in r.flatten() {
                let _ = write!(out, "{rec}");
            }
        }
        out.clear();
    }
});
