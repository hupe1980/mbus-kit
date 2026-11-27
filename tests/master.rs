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
