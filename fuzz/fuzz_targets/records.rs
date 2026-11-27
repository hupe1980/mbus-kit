//! Arbitrary bytes straight into the application layer, every CI the crate knows, and
//! every accessor of every record: description, modifiers, quantity and Display
//! (001-describe-every-record/SR-002, 001-describe-every-record/SR-003), compact
//! profiles and compact-frame expansion (004-compact-and-fragments/SR-005).
#![no_main]

use std::fmt::Write;

use libfuzzer_sys::fuzz_target;
use mbus_kit::{Application, Body};

fuzz_target!(|data: &[u8]| {
    let mut out = String::new();
    for ci in [0x72, 0x7A, 0x78, 0x51] {
        if let Ok(Application { body: Body::Records(mut r), .. }) = Application::parse(ci, data) {
            for rec in r.by_ref().flatten() {
                let d = rec.description();
                let _ = write!(out, "{rec} {d} {:?} {:?}", rec.value, rec.quantity());
                if let Some(p) = rec.profile() {
                    let _ = write!(out, "{p}");
                    for v in p.values(i128::from(i64::MAX)) {
                        let _ = v;
                    }
                }
                for m in rec.vib.modifiers() {
                    let _ = write!(out, "{m} {}", m.keeps_quantity());
                }
                // 001-describe-every-record/SR-003: meter bytes never reach the output unescaped.
                assert!(!out.bytes().any(|b| b.is_ascii_control()), "control byte in {out:?}");
                out.clear();
            }
            let _ = r.manufacturer_data();
        }
    }
    // Compact frames: the input's first half as a layout, the rest as a compact frame.
    let (layout, frame) = data.split_at(data.len() / 2);
    if let Ok(Application { body: Body::Compact(c), .. }) = Application::parse(0x79, frame) {
        let format = mbus_kit::compact::Format { signature: c.signature, headers: layout };
        let mut buf = [0u8; 1024];
        if let Ok(records) = c.expand(&format, &mut buf) {
            for rec in records.flatten() {
                let _ = write!(out, "{rec}");
            }
        }
    }
});
