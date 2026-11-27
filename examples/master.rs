//! Drive a wired M-Bus: read a meter completely (bound to its printed ID), then search
//! the bus.
//!
//! `cargo run --example master`
//!
//! The library is sans-I/O: it says what to send and how long to wait, you move the
//! bytes. Here a scripted bus stands in for the serial port; with a real level converter,
//! `transfer` writes the request, then reads until `wired::scan` finds a frame or the
//! timeout passes.

use mbus_kit::master::{Baud, Event, Outcome, Readout, Search, SecondaryAddress, Target};
use mbus_kit::wired::Frame;

/// What a serial port does for the master: send, then collect the reply.
trait Bus {
    fn transfer(&mut self, request: &[u8], timeout_ms: u32) -> Vec<u8>;
}

fn main() {
    let mut bus = Scripted::default();

    // 1. Read meter 1 completely: SND_NKE, then REQ_UD2 until no more records follow.
    //    Only telegrams from meter 12345678 count (a swapped meter is caught).
    let meter = SecondaryAddress::pattern("12345678").unwrap();
    let mut readout = Readout::new(Target::Primary(1)).expect_meter(meter);
    while let Some(request) = readout.next_request() {
        let reply = bus.transfer(request.bytes(), request.timeout_ms(Baud::B2400));
        let event = if reply.is_empty() { readout.timeout(&reply) } else { readout.reply(&reply) };
        if let Event::Telegram(app) = event {
            println!("telegram from {}", app.meter().map_or("?".into(), |m| m.to_string()));
            for record in app.records().into_iter().flatten().flatten() {
                println!("  {record}");
            }
        }
    }
    match readout.outcome() {
        Some(Outcome::Complete) => println!("complete: {} telegrams\n", readout.telegrams()),
        Some(other) => println!("not usable: {other}\n"),
        None => unreachable!("the readout is over"),
    }

    // 2. Find every meter on the bus by secondary address.
    let mut search = Search::new();
    while let Some(request) = search.next_request() {
        let reply = bus.transfer(request.bytes(), request.timeout_ms(Baud::B2400));
        let event = if reply.is_empty() { search.timeout(&reply) } else { search.reply(&reply) };
        match event {
            Event::Found(meter) => println!("found {meter}"),
            Event::Unreadable(pattern) => println!("{pattern} answers but does not name itself"),
            _ => {}
        }
    }
    println!("{} selections", search.selections());
}

/// One meter at primary address 1 with ID 12345678, sending two telegrams.
#[derive(Default)]
struct Scripted {
    sent: u8,
    selected: bool,
}

impl Bus for Scripted {
    fn transfer(&mut self, request: &[u8], _timeout_ms: u32) -> Vec<u8> {
        match Frame::parse(request) {
            Ok(Some((Frame::Short { c: 0x40, a: 0xFD }, _))) => {
                let was = std::mem::take(&mut self.selected);
                if was { vec![0xE5] } else { Vec::new() }
            }
            Ok(Some((Frame::Short { c: 0x40, .. }, _))) => vec![0xE5],
            Ok(Some((Frame::Long { ci: 0x52, data, .. }, _))) => {
                // A selection matches if every ID nibble is F or ours (12345678).
                let ours = [0x78u8, 0x56, 0x34, 0x12];
                self.selected = data.iter().zip(ours).all(|(&p, o)| {
                    (p >> 4 == 0xF || p >> 4 == o >> 4) && (p & 0xF == 0xF || p & 0xF == o & 0xF)
                });
                if self.selected { vec![0xE5] } else { Vec::new() }
            }
            Ok(Some((Frame::Short { a, .. }, _))) if a == 1 || (a == 0xFD && self.selected) => {
                self.sent += 1;
                let more = self.sent == 1;
                let mut data = vec![0x78, 0x56, 0x34, 0x12, 0x2D, 0x2C, 0x08, 0x04, self.sent, 0, 0, 0];
                data.extend_from_slice(&[0x04, 0x06, 0x1A, 0x92, 0x00, 0x00]); // energy 37.402 MWh
                data.extend_from_slice(if more {
                    &[0x1F][..]
                } else {
                    &[0x04, 0x13, 0xA8, 0x1B, 0x00, 0x00]
                });
                let mut out = [0u8; 64];
                let n = Frame::Long { c: 0x08, a, ci: 0x72, data: &data }.encode(&mut out).unwrap();
                out[..n].to_vec()
            }
            _ => Vec::new(),
        }
    }
}
