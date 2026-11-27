//! Arbitrary bytes into the wireless link layer (every format) and the layer chain.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::wireless::{Format, decode};
use mbus_kit::{Application, Body};

fuzz_target!(|data: &[u8]| {
    for format in [Format::A, Format::B, Format::Stripped] {
        let mut out = [0u8; 512];
        if let Ok(Some((frame, _))) = decode(data, format, &mut out) {
            if let Ok(Application { body: Body::Records(r), .. }) = frame.application() {
                for rec in r.flatten() {
                    let _ = rec.quantity().map(|q| q.to_string());
                }
            }
        }
    }
});
