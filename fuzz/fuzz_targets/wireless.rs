//! Arbitrary bytes into the wireless link layer (both formats).
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::wireless::{Format, decode};
use mbus_kit::{Application, Body};

fuzz_target!(|data: &[u8]| {
    for format in [Format::A, Format::B] {
        let mut out = [0u8; 512];
        if let Ok(Some((frame, _))) = decode(data, format, &mut out) {
            if let Ok(Application { body: Body::Records(r), .. }) = Application::parse(frame.ci, frame.payload) {
                for rec in r.flatten() {
                    let _ = rec.quantity().map(|q| q.to_string());
                }
            }
        }
    }
});
