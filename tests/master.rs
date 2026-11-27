//! The wired master (spec 003) against a simulated bus: meters that answer selections and
//! reads like real ones, including collisions (several identical `E5`s overlap into one
//! clean `E5`; several data replies garble).

use mbus_kit::Address;
use mbus_kit::master::{self, Baud, Event, Readout, Search, SecondaryAddress, Target};

mod sim;
use sim::{Bus, addr, hex};

fn run_search(bus: &mut Bus) -> (Vec<Address>, u32) {
    let mut search = Search::new();
    let mut found = Vec::new();
    while let Some(req) = search.next_request() {
        let reply = bus.send(req.bytes());
        let event = if reply.is_empty() { search.timeout(&[]) } else { search.reply(&reply) };
        if let Event::Found(a) = event {
            found.push(a);
        }
    }
    (found, search.selections())
}

// ---- US3: find every meter (003-wired-master/SR-002)

#[test]
fn search_finds_every_meter_on_buses_with_colliding_prefixes() {
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut rand = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for n in [0usize, 1, 3, 50, 250] {
        let mut ids: Vec<u32> = vec![12_345_678, 12_345_679, 87_654_321];
        ids.truncate(n.min(3));
        while ids.len() < n {
            // Many shared prefixes: cluster IDs around a few bases.
            let base = [12_340_000u32, 12_345_600, 99_000_000][(rand() % 3) as usize];
            let id = base + (rand() % 10_000) as u32;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut bus = Bus::new(&ids);
        let (found, selections) = run_search(&mut bus);
        let mut got: Vec<u32> = found.iter().map(|a| a.id_number().unwrap()).collect();
        got.sort_unstable();
        let mut want = ids.clone();
        want.sort_unstable();
        assert_eq!(got, want, "bus of {n}");
        let bound = 10 * 8 * (u32::try_from(n).unwrap() + 1) * u32::from(master::RETRIES + 1);
        assert!(selections <= bound, "bus of {n}: {selections} selections > {bound}");
    }
}

#[test]
fn two_meters_with_one_id_are_reported_not_dropped() {
    let mut bus = Bus::new(&[12_345_678, 12_345_678, 55_555_555]);
    bus.meters[1].address.version = 2;
    let mut search = Search::new();
    let (mut found, mut ambiguous) = (Vec::new(), Vec::new());
    while let Some(req) = search.next_request() {
        let reply = bus.send(req.bytes());
        match if reply.is_empty() { search.timeout(&[]) } else { search.reply(&reply) } {
            Event::Found(a) => found.push(a.id_number().unwrap()),
            Event::Ambiguous(s) => ambiguous.push(s.to_string()),
            _ => {}
        }
    }
    assert_eq!(found, [55_555_555]);
    assert_eq!(ambiguous, ["12345678"]);
}

#[test]
fn trailing_bytes_after_an_ack_are_a_collision() {
    let mut search = Search::new();
    search.next_request();
    assert!(matches!(search.reply(&[0xE5, 0xE5]), Event::Retrying));
    assert_eq!(search.probe().to_string(), "00FFFFFF", "narrowed to the next digit");
}

#[test]
fn empty_bus_probes_each_first_digit() {
    let mut bus = Bus::new(&[]);
    let (found, selections) = run_search(&mut bus);
    assert!(found.is_empty());
    assert_eq!(selections, 10);
}

// ---- US1: multi-telegram readout with the FCB (003-wired-master/SR-003)

#[test]
fn readout_reads_every_telegram_and_toggles_the_fcb() {
    let mut bus = Bus::new(&[12_345_678]);
    bus.meters[0].telegrams = 3;
    let mut readout = Readout::new(Target::Primary(1));
    let mut volumes = Vec::new();
    while let Some(req) = readout.next_request() {
        let reply = bus.send(req.bytes());
        match readout.reply(&reply) {
            Event::Telegram(app) => {
                for r in app.records().unwrap() {
                    volumes.push(r.unwrap().value.as_integer().unwrap());
                }
            }
            Event::Acknowledged => {}
            other => panic!("{other:?}"),
        }
    }
    assert!(readout.is_complete());
    assert_eq!(volumes, [0, 1, 2]);
    // SND_NKE, then REQ_UD2 with FCB 1, 0, 1 (C = 7B, 5B, 7B).
    let c: Vec<u8> = bus.log.iter().map(|f| f[1]).collect();
    assert_eq!(c, [0x40, 0x7B, 0x5B, 0x7B]);
}

#[test]
fn readout_repeats_with_the_same_fcb_after_a_lost_reply() {
    let mut bus = Bus::new(&[12_345_678]);
    bus.meters[0].telegrams = 2;
    let mut readout = Readout::new(Target::Primary(1));
    let mut lost = false;
    let mut volumes = Vec::new();
    while let Some(req) = readout.next_request() {
        let mut reply = bus.send(req.bytes());
        if !lost && reply.len() > 1 {
            lost = true;
            reply.clear(); // the first data reply is lost on the line
        }
        let event = if reply.is_empty() { readout.timeout(&[]) } else { readout.reply(&reply) };
        if let Event::Telegram(app) = event {
            for r in app.records().unwrap() {
                volumes.push(r.unwrap().value.as_integer().unwrap());
            }
        }
    }
    assert_eq!(volumes, [0, 1], "the meter resends telegram 0 for the repeated FCB");
    let c: Vec<u8> = bus.log.iter().map(|f| f[1]).collect();
    assert_eq!(c, [0x40, 0x7B, 0x7B, 0x5B]);
}

// 003-wired-master/SR-001
#[test]
fn readout_gives_up_after_the_retries() {
    let mut readout = Readout::new(Target::Primary(9)).retries(2);
    let mut sent = 0;
    let mut last = None;
    while readout.next_request().is_some() {
        sent += 1;
        last = Some(readout.timeout(&[]));
    }
    assert_eq!(sent, 3);
    assert!(matches!(last, Some(Event::NoReply)));
    assert!(!readout.is_complete());
}

// ---- US2: secondary addressing

#[test]
fn readout_by_secondary_address_selects_reads_and_deselects() {
    let mut bus = Bus::new(&[12_345_678, 87_654_321]);
    let mut readout = Readout::new(Target::Secondary(SecondaryAddress::id(87_654_321).unwrap()));
    let mut meter = None;
    while let Some(req) = readout.next_request() {
        let reply = bus.send(req.bytes());
        let event = if reply.is_empty() { readout.timeout(&[]) } else { readout.reply(&reply) };
        if let Event::Telegram(app) = event {
            meter = app.meter();
        }
    }
    assert_eq!(meter.unwrap().id_number(), Some(87_654_321));
    assert!(readout.is_complete());
    assert!(bus.meters.iter().all(|m| !m.selected), "deselected after the read");
}

#[test]
fn secondary_address_patterns() {
    let s = SecondaryAddress::pattern("12F45678").unwrap();
    assert_eq!(s.id_bytes(), [0x78, 0x56, 0xF4, 0x12]);
    assert_eq!(s.to_string(), "12F45678");
    assert!(s.matches(&addr(12_345_678)));
    assert!(!s.matches(&addr(12_345_679)));
    assert_eq!(SecondaryAddress::pattern("1234").unwrap().to_string(), "1234FFFF");
    assert!(SecondaryAddress::pattern("123456789").is_none());
    assert!(SecondaryAddress::pattern("12x").is_none());
    let exact = SecondaryAddress::exact(&addr(12_345_678));
    assert_eq!(exact.to_string(), "12345678 KAM v1 heat (outlet)");
    assert_eq!(SecondaryAddress::ANY.to_bytes(), [0xFF; 8]);
}

// ---- US4: commands, and timing

#[test]
fn command_frames() {
    // Set primary address 5 → 17: 68 06 06 68 53 05 51 01 7A 11 CS 16.
    let f = master::set_primary_address(5, 17).unwrap();
    assert_eq!(f.bytes(), hex("68060668530551017A113516"));
    assert!(master::set_primary_address(5, 0xFD).is_none());
    // Reference frames (m-bus.com chapter 6, libmbus, pyMeterBus; checksums verified).
    assert_eq!(master::set_primary_address(0xFE, 8).unwrap().bytes(), hex("6806066853FE51017A082516"));
    assert_eq!(master::application_reset(0xFE, Some(0x10)).bytes(), hex("6804046853FE5010B116"));
    assert_eq!(
        master::application_reset(1, None).bytes(),
        hex("680303685301 50A416".replace(' ', "").as_str())
    );
    assert_eq!(master::set_baud(0xFE, Baud::B9600).bytes(), hex("6803036853FEBD0E16"));
    assert_eq!(master::request_all_data(3).bytes(), hex("68040468530351 7F2616".replace(' ', "").as_str()));
    let t = mbus_kit::DateTime::new(2026, 11, 27, 9, 36).unwrap();
    assert!(mbus_kit::DateTime::new(2026, 2, 30, 9, 36).is_none());
    assert_eq!(
        master::set_time(1, &t).bytes(),
        hex("6809096853015104 6D24095B3BD916".replace(' ', "").as_str())
    );
    assert_eq!(master::link_reset(0xFD).bytes(), hex("1040FD3D16"));
    // Selections carry FCB = 1 (libmbus, pyMeterBus); the spec figure's C = 53 is FCB = 0.
    assert_eq!(master::select(&SecondaryAddress::ANY).bytes(), hex("680B0B6873FD52FFFFFFFFFFFFFFFFBA16"));
    let sel = master::select(&SecondaryAddress::id(12_345_678).unwrap());
    assert_eq!(sel.bytes()[7..15], [0x78, 0x56, 0x34, 0x12, 0xFF, 0xFF, 0xFF, 0xFF]);
    let enhanced = master::select_enhanced(&SecondaryAddress::id(12_345_678).unwrap(), Some(1_020_304));
    assert_eq!(
        enhanced.with_fcb(false).bytes(),
        hex("68111168 53FD52 78563412 FFFFFFFF 0C78 04030201 4016".replace(' ', "").as_str())
    );
    let wild = master::select_enhanced(&SecondaryAddress::id(12_345_678).unwrap(), None).with_fcb(false);
    assert_eq!(wild.bytes(), hex("6811116853FD5278563412FFFFFFFF0C78FFFFFFFF3216"));
    // with_fcb recomputes the checksum: the spec's C = 53 selection of the same ID.
    assert_eq!(
        master::select(&SecondaryAddress::id(12_345_678).unwrap()).with_fcb(false).bytes(),
        hex("680B0B6853FD5278563412FFFFFFFFB216")
    );
    // Broadcast expects no reply.
    assert!(!master::link_reset(0xFF).expects_reply());
    // 330 bit times + 50 ms: 188 ms at 2400 Bd, 85 ms at 9600 Bd (the 50 ms do not scale).
    assert_eq!(master::response_timeout_ms(Baud::B2400), 188);
    assert_eq!(master::link_reset(1).timeout_ms(Baud::B2400), 188);
    assert_eq!(master::link_reset(1).timeout_ms(Baud::B9600), 85);
    assert_eq!(master::link_reset(1).timeout_ms(Baud::B300), 1150);
    assert_eq!(master::link_reset(0xFF).timeout_ms(Baud::B2400), 0);
}

#[test]
fn a_line_where_everything_collides_is_given_up_not_searched_forever() {
    // 005-io-shells/SR-001: 10⁸ prefixes collide on a babbling line; the limit ends it.
    let mut search = Search::new().max_selections(500);
    let mut n = 0u32;
    while let Some(_req) = search.next_request() {
        let _ = search.reply(&[0x13, 0x37]);
        n += 1;
    }
    assert!(n <= 500 + 1, "{n}");
    assert!(!search.is_complete());
    let mut empty = Search::new();
    while empty.next_request().is_some() {
        let _ = empty.timeout(&[]);
    }
    assert!(empty.is_complete());
}

// ---- spec 007: readout integrity

use mbus_kit::master::Outcome;
use mbus_kit::wired::Frame;

/// A long-header RSP_UD with control `c` from primary `a`, meter `id` (KAM, heat), one
/// volume record `v`, and `1F` when more follows; `tail` are extra record bytes.
fn page(c: u8, a: u8, id: u32, v: u8, more: bool, tail: &[u8]) -> Vec<u8> {
    let m = addr(id);
    let [i0, i1, i2, i3] = m.id;
    let [m0, m1] = m.manufacturer.0.to_le_bytes();
    let mut data = vec![i0, i1, i2, i3, m0, m1, m.version, m.medium.0, v, 0, 0, 0, 0x04, 0x13, v, 0, 0, 0];
    data.extend_from_slice(tail);
    if more {
        data.push(0x1F);
    }
    let mut out = [0u8; 300];
    let n = Frame::Long { c, a, ci: 0x72, data: &data }.encode(&mut out).unwrap();
    out[..n].to_vec()
}

/// Drive `readout` with `replies` (empty = timeout; exhausted = timeouts). Returns the
/// events and the requests' C fields.
fn drive(readout: &mut Readout, replies: &[Vec<u8>]) -> (Vec<String>, Vec<u8>) {
    let mut events = Vec::new();
    let mut sent = Vec::new();
    let mut it = replies.iter();
    while let Some(req) = readout.next_request() {
        assert!(sent.len() < 100, "readout does not end");
        sent.push(req.bytes()[if req.bytes()[0] == 0x68 { 4 } else { 1 }]);
        let reply = it.next().map_or(&[][..], Vec::as_slice);
        let event = if reply.is_empty() { readout.timeout(reply) } else { readout.reply(reply) };
        events.push(match event {
            Event::Telegram(app) => format!("T{}", app.meter().unwrap().id_number().unwrap()),
            other => format!("{other:?}").split('(').next().unwrap().to_string(),
        });
    }
    (events, sent)
}

const ACK: &[u8] = &[0xE5];

// 007-readout-integrity/SR-002
#[test]
fn readout_limit_reached_is_not_complete() {
    // The issue's case: a two-telegram meter, max_telegrams(1).
    let mut r = Readout::new(Target::Primary(5)).max_telegrams(1);
    let (events, sent) = drive(&mut r, &[ACK.to_vec(), page(0x08, 5, 1, 0, true, &[])]);
    assert_eq!(events, ["Acknowledged", "T1"]);
    assert_eq!(sent, [0x40, 0x7B], "no second REQ_UD2");
    assert_eq!(r.outcome(), Some(Outcome::LimitReached));
    assert!(!r.is_complete());
    assert_eq!(r.telegrams(), 1);
    // max_telegrams(0) reads one telegram, like 1.
    let mut r = Readout::new(Target::Primary(5)).max_telegrams(0);
    drive(&mut r, &[ACK.to_vec(), page(0x08, 5, 1, 0, true, &[])]);
    assert_eq!(r.outcome(), Some(Outcome::LimitReached));
    // Without the limit both pages are read and the readout is complete.
    let mut r = Readout::new(Target::Primary(5));
    let (events, _) =
        drive(&mut r, &[ACK.to_vec(), page(0x08, 5, 1, 0, true, &[]), page(0x08, 5, 1, 1, false, &[])]);
    assert_eq!(events, ["Acknowledged", "T1", "T1"]);
    assert_eq!(r.outcome(), Some(Outcome::Complete));
}

// 007-readout-integrity/SR-002, 007-readout-integrity/SR-004
#[test]
fn a_secondary_readout_that_ends_early_still_deselects() {
    let s = Target::Secondary(SecondaryAddress::id(1).unwrap());
    let cases: [(Vec<Vec<u8>>, Outcome); 3] = [
        (vec![ACK.to_vec(), ACK.to_vec(), page(0x08, 0, 1, 0, true, &[])], Outcome::LimitReached),
        (
            vec![ACK.to_vec(), ACK.to_vec(), page(0x08, 0, 2, 0, false, &[])],
            Outcome::WrongMeter(Some(addr(2))),
        ),
        (vec![ACK.to_vec(), ACK.to_vec()], Outcome::NoReply),
    ];
    for (replies, outcome) in cases {
        let mut r = Readout::new(s).max_telegrams(1);
        let (_, sent) = drive(&mut r, &replies);
        assert_eq!(sent.last(), Some(&0x40), "{outcome:?}: ends with SND_NKE to FD");
        assert_eq!(r.outcome(), Some(outcome));
    }
    // An undecodable telegram (CI 72 cut short) too, and the outcome says why.
    let mut short = [0u8; 32];
    let n = Frame::Long { c: 0x08, a: 0, ci: 0x72, data: &[0x78, 0x56] }.encode(&mut short).unwrap();
    let mut r = Readout::new(s);
    let (events, sent) = drive(&mut r, &[ACK.to_vec(), ACK.to_vec(), short[..n].to_vec()]);
    assert_eq!(events[2], "Invalid");
    assert_eq!(sent.last(), Some(&0x40));
    assert!(matches!(r.outcome(), Some(Outcome::Invalid(_))));
}

#[test]
fn outcome_is_none_while_running_and_e5_rules() {
    let mut r = Readout::new(Target::Primary(5));
    assert_eq!(r.outcome(), None);
    r.next_request();
    r.reply(ACK);
    assert_eq!(r.outcome(), None);
    // E5 after a page that said "more follows": nothing more, complete.
    let mut r = Readout::new(Target::Primary(5));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), page(0x08, 5, 1, 0, true, &[]), ACK.to_vec()]);
    assert_eq!(events, ["Acknowledged", "T1", "Acknowledged"]);
    assert!(r.is_complete());
    // An E5 before the first page is a late or doubled acknowledgement, never "complete".
    let mut r = Readout::new(Target::Primary(5));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), ACK.to_vec(), page(0x08, 5, 1, 0, false, &[])]);
    assert_eq!(events, ["Acknowledged", "Retrying", "T1"]);
    assert!(r.is_complete());
    let mut r = Readout::new(Target::Primary(5));
    drive(&mut r, &[ACK.to_vec(), ACK.to_vec(), ACK.to_vec(), ACK.to_vec()]);
    assert_eq!(r.outcome(), Some(Outcome::NoReply));
    // after the end, a timeout is no retry.
    assert!(matches!(r.timeout(&[]), Event::NoReply));
}

// 007-readout-integrity/SR-001
#[test]
fn only_the_addressed_slave_can_answer() {
    let rejected = [
        page(0x08, 6, 1, 0, false, &[]), // another primary address
        page(0x53, 5, 1, 0, false, &[]), // a master's SND_UD
        page(0x48, 5, 1, 0, false, &[]), // PRM set
        page(0x0A, 5, 1, 0, false, &[]), // another function
        [page(0x08, 5, 1, 0, false, &[]), vec![0x00]].concat(), // a byte after the frame
    ];
    for bad in rejected {
        let mut r = Readout::new(Target::Primary(5));
        let (events, _) = drive(&mut r, &[ACK.to_vec(), bad.clone(), bad.clone(), bad]);
        assert_eq!(events, ["Acknowledged", "Retrying", "Retrying", "NoReply"]);
        assert_eq!(r.outcome(), Some(Outcome::NoReply));
    }
    // After a foreign frame the real reply still counts (same FCB repeated).
    let mut r = Readout::new(Target::Primary(5));
    let (events, sent) =
        drive(&mut r, &[ACK.to_vec(), page(0x08, 6, 9, 0, false, &[]), page(0x38, 5, 1, 0, false, &[])]);
    assert_eq!(events, ["Acknowledged", "Retrying", "T1"], "ACD and DFC set: still a response");
    assert_eq!(sent, [0x40, 0x7B, 0x7B]);
    assert!(r.is_complete());
    // 254: the slave answers with its own address.
    let mut r = Readout::new(Target::Primary(254));
    drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 1, 0, false, &[])]);
    assert!(r.is_complete());
    // A trailing byte after the E5 of the reset is garbled too.
    let mut r = Readout::new(Target::Primary(5));
    r.next_request();
    assert!(matches!(r.reply(&[0xE5, 0xE5]), Event::Retrying));
}

// 007-readout-integrity/SR-003
#[test]
fn one_readout_one_meter() {
    let kam = |id| SecondaryAddress::exact(&addr(id));
    // Bound to the printed identity: another meter at the same address is refused.
    let mut r = Readout::new(Target::Primary(17)).expect_meter(kam(6_855_817));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 6_855_818, 0, false, &[])]);
    assert_eq!(events, ["Acknowledged", "WrongMeter"]);
    assert_eq!(r.outcome(), Some(Outcome::WrongMeter(Some(addr(6_855_818)))));
    assert_eq!(r.telegrams(), 0);
    // Wildcards in the binding.
    let mut r =
        Readout::new(Target::Primary(17)).expect_meter(SecondaryAddress::pattern("0685FFFF").unwrap());
    drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 6_855_818, 0, false, &[])]);
    assert!(r.is_complete());
    // A secondary readout is bound to its selection.
    let mut r = Readout::new(Target::Secondary(SecondaryAddress::pattern("1234FFFF").unwrap()));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), ACK.to_vec(), page(0x08, 0, 55_555_555, 0, false, &[])]);
    assert_eq!(events[2], "WrongMeter");
    // Unbound: page 2 must come from page 1's meter.
    let mut r = Readout::new(Target::Primary(17));
    let (events, _) =
        drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 1, 0, true, &[]), page(0x08, 17, 2, 1, false, &[])]);
    assert_eq!(events, ["Acknowledged", "T1", "WrongMeter"]);
    assert_eq!(r.outcome(), Some(Outcome::WrongMeter(Some(addr(2)))));
    // A telegram without identity (short header, CI 7A) under a binding.
    let mut short = [0u8; 32];
    let n = Frame::Long { c: 0x08, a: 17, ci: 0x7A, data: &[1, 0, 0, 0, 0x04, 0x13, 1, 0, 0, 0] }
        .encode(&mut short)
        .unwrap();
    let mut r = Readout::new(Target::Primary(17)).expect_meter(kam(1));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), short[..n].to_vec()]);
    assert_eq!(events, ["Acknowledged", "WrongMeter"]);
    assert_eq!(r.outcome(), Some(Outcome::WrongMeter(None)));
}

// 007-readout-integrity/SR-003 (SC-003): what the ems driver checks by hand.
#[test]
fn the_ems_heat_meter_readout_is_configuration() {
    let ems = || {
        Readout::new(Target::Primary(17))
            .expect_meter(SecondaryAddress::exact(&addr(6_855_817)))
            .max_telegrams(1)
    };
    let mut r = ems();
    drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 6_855_817, 0, false, &[])]);
    assert!(r.is_complete());
    let mut r = ems();
    drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 6_855_817, 0, true, &[])]);
    assert_eq!(r.outcome(), Some(Outcome::LimitReached), "a paginating meter is not a total");
    let mut r = ems();
    drive(&mut r, &[ACK.to_vec(), page(0x08, 17, 6_855_818, 0, false, &[])]);
    assert!(matches!(r.outcome(), Some(Outcome::WrongMeter(_))));
}

// A broken record before `1F` hides "more follows": not complete.
#[test]
fn a_broken_record_is_not_a_complete_readout() {
    let mut r = Readout::new(Target::Primary(5));
    let (events, _) = drive(&mut r, &[ACK.to_vec(), page(0x08, 5, 1, 0, true, &[0x6F])]);
    assert_eq!(events, ["Acknowledged", "T1"], "delivered: its records show the error");
    assert_eq!(r.outcome(), Some(Outcome::Invalid(mbus_kit::Error::ReservedDif(0x6F))));
}

// 251, 252 and 255 never answer; nothing is sent.
#[test]
fn reserved_primary_addresses_are_refused_up_front() {
    for a in [251, 252, 255] {
        let mut r = Readout::new(Target::Primary(a));
        assert!(r.next_request().is_none());
        assert_eq!(r.outcome(), Some(Outcome::Invalid(mbus_kit::Error::InvalidAddress(a))));
    }
}

// a meter that acknowledges a selection but does not name itself is reported.
#[test]
fn search_reports_meters_it_cannot_read() {
    let run = |read: &dyn Fn(&[u8]) -> Vec<u8>| {
        let mut search = Search::new();
        let mut events = Vec::new();
        while let Some(req) = search.next_request() {
            let b = req.bytes();
            let reply = if b[0] == 0x68 {
                // One meter, 12345678: it acknowledges the selections that match it.
                let pattern: [u8; 8] = b[7..15].try_into().unwrap();
                if sim::matches(&pattern, &addr(12_345_678)) { vec![0xE5] } else { vec![] }
            } else {
                read(b)
            };
            match if reply.is_empty() { search.timeout(&[]) } else { search.reply(&reply) } {
                Event::Found(a) => events.push(format!("found {}", a.id_number().unwrap())),
                Event::Unreadable(p) => events.push(format!("unreadable {p}")),
                _ => {}
            }
        }
        assert!(search.is_complete());
        events
    };
    let ok = |_: &[u8]| page(0x08, 0, 12_345_678, 0, false, &[]);
    assert_eq!(run(&ok), ["found 12345678"]);
    // Short header: no identity.
    let short = |_: &[u8]| hex("680808680800780313153100DC16");
    let no_id = run(&short);
    // Another ID than the probe (S3), an E5 to the read (S6), silence (S2).
    let other = |_: &[u8]| page(0x08, 0, 99_999_999, 0, false, &[]);
    let ack = |_: &[u8]| vec![0xE5];
    let silent = |_: &[u8]| vec![];
    for (name, events) in
        [("short", no_id), ("other", run(&other)), ("e5", run(&ack)), ("silent", run(&silent))]
    {
        assert!(events.iter().all(|e| e.starts_with("unreadable")), "{name}: {events:?}");
        assert!(!events.is_empty(), "{name}: dropped silently");
    }
}
