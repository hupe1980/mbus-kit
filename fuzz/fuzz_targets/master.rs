//! Arbitrary replies into the master state machines (spec 003): a readout and a search
//! must handle any bytes and always end (003-wired-master/SR-001). Each reply is `len` + bytes; length 0 is a
//! timeout.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::master::{self, Event, Readout, Search, SecondaryAddress, Target};
use mbus_kit::wired;

fn replies(mut data: &[u8]) -> impl Iterator<Item = &[u8]> {
    std::iter::from_fn(move || {
        let (&n, rest) = data.split_first()?;
        let n = usize::from(n).min(rest.len());
        let (reply, rest) = rest.split_at(n);
        data = rest;
        Some(reply)
    })
}

fuzz_target!(|data: &[u8]| {
    let _ = wired::scan(data);
    // 007-readout-integrity/SR-001, 007-readout-integrity/SR-005: every telegram passes the reply rules, and
    // the readout ends within (limit + 3) × (retries + 1) requests.
    for target in [Target::Primary(5), Target::Secondary(SecondaryAddress::ANY)] {
        let mut input = replies(data);
        let mut readout = Readout::new(target).max_telegrams(4);
        let mut steps = 0;
        while readout.next_request().is_some() {
            steps += 1;
            assert!(steps <= (4 + 3) * (usize::from(master::RETRIES) + 1), "readout does not end");
            let reply = input.next().unwrap_or(&[]); // input exhausted: timeouts
            let mut event = if reply.is_empty() { readout.timeout(reply) } else { readout.reply(reply) };
            if let Event::NeedMore = event {
                // The window closed on an incomplete frame (what serial::Bus does).
                event = readout.timeout(reply);
            }
            if let Event::Telegram(app) = event {
                let Ok(Some((wired::Frame::Long { c, a, .. }, used))) = wired::Frame::parse(reply) else {
                    panic!("a telegram from something that is not one long frame");
                };
                assert_eq!(used, reply.len(), "bytes after the frame");
                assert!(wired::control::is_response(c), "not a slave response: C {c:02X}");
                assert!(target != Target::Primary(5) || a == 5, "from primary {a}");
                for r in app.records().into_iter().flatten().flatten() {
                    let _ = r.to_string();
                }
            }
        }
        assert!(readout.outcome().is_some(), "over without an outcome");
    }
    let mut input = replies(data);
    let mut search = Search::new();
    let mut steps = 0u32;
    while search.next_request().is_some() {
        steps += 1;
        // 10 × 8 selections per level, 3 tries each, one read per found meter: a generous cap.
        assert!(steps < 100_000, "search does not end");
        let reply = input.next().unwrap_or(&[]); // input exhausted: timeouts
        let _ = if reply.is_empty() { search.timeout(reply) } else { search.reply(reply) };
    }
});
