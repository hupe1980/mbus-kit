//! Arbitrary bytes straight into the application layer, every CI the crate knows.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::{Application, Body};

fuzz_target!(|data: &[u8]| {
    for ci in [0x72, 0x7A, 0x78, 0x51] {
        if let Ok(Application { body: Body::Records(mut r), .. }) = Application::parse(ci, data) {
            for rec in r.by_ref().flatten() {
                let _ = format!("{} {:?} {:?}", rec.name(), rec.value, rec.quantity());
            }
            let _ = r.manufacturer_data();
        }
    }
});
