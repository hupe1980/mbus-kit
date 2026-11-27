//! Arbitrary bytes through every stick decoder (005-io-shells/SR-002), fed in chunks whose
//! size the first byte picks; every telegram goes on into the wireless decoder.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::stick::{Message, Receiver, Stick};
use mbus_kit::wireless::{self, Format};

fuzz_target!(|data: &[u8]| {
    let Some((&size, data)) = data.split_first() else { return };
    let chunk = usize::from(size % 64) + 1;
    for stick in [Stick::Im871a, Stick::Iu891a, Stick::Amb8465] {
        let mut rx = Receiver::new(stick);
        for piece in data.chunks(chunk) {
            let mut rest = piece;
            let mut rounds = 0;
            loop {
                let taken = rx.extend(rest);
                rest = &rest[taken..];
                while let Some(m) = rx.next_message() {
                    if let Ok(Message::Telegram(t)) = m {
                        let mut buf = [0u8; 600];
                        let _ = wireless::decode(t.frame, Format::Stripped, &mut buf);
                    }
                }
                rounds += 1;
                assert!(rounds < 4, "the receiver must make room");
                if rest.is_empty() {
                    break;
                }
            }
        }
    }
});
