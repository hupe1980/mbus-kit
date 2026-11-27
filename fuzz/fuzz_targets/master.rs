//! Arbitrary replies into the master state machines (spec 003): a readout and a search
//! must handle any bytes and always end (003-wired-master/SR-001). Each reply is `len` + bytes; length 0 is a
//! timeout.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::master::{Event, Readout, Search, SecondaryAddress, Target};
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
    let mut input = replies(data);
    let mut readout = Readout::new(Target::Secondary(SecondaryAddress::ANY));
    let mut steps = 0;
    while readout.next_request().is_some() {
        steps += 1;
        assert!(steps < 200, "readout does not end");
        let reply = input.next().unwrap_or(&[]); // input exhausted: timeouts
        if let Event::Telegram(app) = if reply.is_empty() { readout.timeout(reply) } else { readout.reply(reply) } {
            for r in app.records().into_iter().flatten().flatten() {
                let _ = r.to_string();
            }
        }
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
