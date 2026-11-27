//! Arbitrary bytes into the wired link layer, every long frame into the application layer.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::wired::Frame;
use mbus_kit::{Application, Body};

fuzz_target!(|data: &[u8]| {
    if let Ok(Some((Frame::Long { ci, data, .. }, _))) = Frame::parse(data) {
        if let Ok(Application { body: Body::Records(r), .. }) = Application::parse(ci, data) {
            for rec in r.flatten() {
                let _ = rec.quantity().map(|q| q.to_string());
            }
        }
    }
});
