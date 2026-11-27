//! AFL fragment reassembly (spec 004 US3) against OMS Vol. 2 Annex N Release E: N.3
//! (wireless, mode 7 with CMAC), N.4 (wired, mode 5) and N.5 (wireless, mode 10), three
//! fragments each, as printed (format A with CRCs; wired frames with checksum).
#![cfg(feature = "oms")]

use mbus_kit::app::Authenticity;
use mbus_kit::fragment::Reassembly;
use mbus_kit::oms::Key;
use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body, Error};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

const KEY: &str = "000102030405060708090A0B0C0D0E0F";

const N3: [&str; 3] = [
    "39 08 49 6A 78 56 34 12 01 07 14 64 8E 80 11 3A 63 66 55 44 33 0A 31 90 09 01 78 65 D4 38 B3 0A 00 00 56 00 7A 05 00 50 07 10 30 53 9A 7C 63 AB DB 1C BC A6 D4 3C B0 2D 76 2A 1E 16 26 FE EF 0E 8E 95",
    "39 08 49 6A 78 56 34 12 01 07 14 64 8E 80 12 3A 63 66 55 44 33 0A 31 90 02 02 40 C4 E9 B3 90 27 8E 41 A4 8B AD 14 38 BD E3 8D 4D 11 66 30 21 5F 5D EF F6 39 2B 6B E3 1A 9F C8 12 75 7B E8 05 B4 B1 86",
    "3B 08 49 6A 78 56 34 12 01 07 63 42 8E 80 13 3A 63 66 55 44 33 0A 31 90 0A 03 04 BE 41 AF 47 ED 4C 9C C1 1A 78 06 CC 3E 04 57 C7 25 B4 B2 CD 8C 9B E7 FE F0 78 77 71 87 CC EF 8E 2A F5 1C C7 29 95 83 EF 7A C7 F2",
];

const N4: [&str; 3] = [
    "68 2A 2A 68 08 03 90 05 01 70 40 5D 00 72 78 56 34 12 93 44 10 07 05 00 50 05 28 FC B7 63 E5 1B 4A 6D 4F DD F2 EE A9 06 F6 1E D0 DA 7A B2 16 16",
    "68 27 27 68 08 03 90 02 02 40 97 87 E1 B2 B5 E3 4E F3 C5 90 3E 3A E4 24 27 CD A9 DB 24 07 FA 81 31 EF B2 25 97 98 E2 B7 9B AA D1 31 16",
    "68 21 21 68 08 03 90 02 03 00 AF 89 B7 50 6F EB 16 C2 2B 15 1B 35 37 FA 27 2D 55 22 75 62 C6 3F 6C 40 83 83 EB 16 16",
];

const N5: [&str; 3] = [
    "38 08 A7 3D 78 56 34 12 01 07 60 9E 8E 80 11 3A 63 66 55 44 33 0A 31 90 05 01 70 40 1E 8C 5C 00 7A 05 00 49 2A 10 01 B3 0A 00 00 02 B1 2B DD B9 47 A6 B8 9C F8 F5 4D E4 BF C8 9C F8 36 12 08 20 60",
    "39 08 A7 3D 78 56 34 12 01 07 5B 0D 8E 80 12 3A 63 66 55 44 33 0A 31 90 02 02 40 85 E9 E0 29 0B 67 5F E6 DB 08 EE 8B 55 81 29 89 10 8B 08 87 6B 13 1C EF 21 45 AC 23 2C 9C E2 43 50 51 81 F0 12 DE BE",
    "36 08 A7 3D 78 56 34 12 01 07 18 09 8E 80 13 3A 63 66 55 44 33 0A 31 90 02 03 00 94 83 99 C5 2A B9 8A 7E C5 1D B7 CD C0 2F 51 E1 7A AB 87 D9 9E C6 B2 59 D2 8B E4 13 70 F8 74 1B 9D 99 27 4E",
];

/// The values N.3 and N.4 document (the same water meter message).
const WATER: [&str; 7] = [
    "volume — 411.979 m³",
    "date — 2013-08-18",
    "volume · storage 1 — 383.294 m³",
    "date · storage 8 — 2012-01-01",
    "volume · storage 8 — 345.290 m³",
    "volume · compact profile · storage 8 — 11 differences, every month",
    "error flags — 0",
];

fn records(app: &Application<'_>) -> Vec<String> {
    app.records().unwrap().map(|r| r.unwrap().to_string()).collect()
}

/// Push every wireless fragment; the last yields the message, decrypted with `KEY`.
fn reassemble_wireless(frames: &[&str]) -> (Authenticity, Vec<String>) {
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    let key = Key::from_hex(KEY).unwrap();
    for (i, f) in frames.iter().enumerate() {
        let mut buf = [0u8; 128];
        let (frame, _) = wireless::decode(&hex(f), Format::A, &mut buf).unwrap().unwrap();
        let app = frame.application().unwrap();
        assert!(matches!(app.body, Body::Fragment(_)), "fragment {i}: {:?}", app.body);
        assert_eq!(app.afl.unwrap().fragment_id(), u8::try_from(i + 1).unwrap());
        match joined.push(&app).unwrap() {
            None => assert!(i + 1 < frames.len()),
            Some(message) => {
                assert_eq!(i + 1, frames.len());
                let Body::Encrypted(e) = message.body else { panic!("{:?}", message.body) };
                let mut plain = [0u8; 256];
                let open = e.decrypt(&key, &mut plain).unwrap();
                return (open.authenticity, records(&open));
            }
        }
    }
    panic!("no last fragment")
}

#[test]
fn annex_n_3_wireless_mode_7_fragments_are_authenticated_by_the_last_fragments_mac() {
    // 004-compact-and-fragments/SR-002
    let (authenticity, values) = reassemble_wireless(&N3);
    // The MAC covers MCL ‖ MCR ‖ ML ‖ the whole 86-byte message.
    assert_eq!(authenticity, Authenticity::Authenticated { counter: 2739 });
    assert_eq!(values, WATER);
}

#[test]
fn annex_n_5_wireless_mode_10_fragments_are_authenticated_by_the_ccm_tag() {
    // 004-compact-and-fragments/SR-004
    let (authenticity, values) = reassemble_wireless(&N5);
    assert_eq!(authenticity, Authenticity::Authenticated { counter: 2739 });
    assert_eq!(values[0], "volume — 411.979 m³");
}

#[test]
fn annex_n_4_wired_mode_5_fragments() {
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    let mut done = None;
    for f in N4 {
        let bytes = hex(f);
        let (Frame::Long { ci, data, .. }, _) = Frame::parse(&bytes).unwrap().unwrap() else { panic!() };
        let app = Application::parse(ci, data).unwrap();
        if let Some(message) = joined.push(&app).unwrap() {
            let Body::Encrypted(e) = message.body else { panic!() };
            let mut plain = [0u8; 256];
            let open = e.decrypt(&Key::from_hex(KEY).unwrap(), &mut plain).unwrap();
            assert_eq!(open.authenticity, Authenticity::Encrypted, "mode 5 has no MAC");
            // The monthly history from the base value (storage 8, 2012-01-01) onwards.
            let all: Vec<_> = open.records().unwrap().map(Result::unwrap).collect();
            let base = all[4].value.as_integer().unwrap();
            let history: Vec<_> = all[5].profile().unwrap().values(base).collect();
            // Element 1 is February: 345290 + 2660 l; differences are BCD 002660, 003939, …
            assert_eq!((history[0], history[10]), (Some(347_950), Some(381_672)));
            done = Some(records(&open));
        }
    }
    assert_eq!(done.unwrap(), WATER);
}

/// The N.3 fragments, decoded once.
fn n3_apps(bufs: &mut [[u8; 128]; 3]) -> Vec<Application<'_>> {
    bufs.iter_mut()
        .zip(N3)
        .map(|(buf, f)| wireless::decode(&hex(f), Format::A, buf).unwrap().unwrap().0.application().unwrap())
        .collect()
}

#[test]
fn missing_repeated_and_out_of_order_fragments_are_reported_and_the_message_kept() {
    // 004-compact-and-fragments/SR-003
    let mut bufs = [[0u8; 128]; 3];
    let apps = n3_apps(&mut bufs);
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    // Nothing open: only fragment 1 may start.
    assert_eq!(joined.push(&apps[1]).unwrap_err(), Error::UnexpectedFragment { expected: 1, found: 2 });
    assert!(joined.push(&apps[0]).unwrap().is_none());
    // Missing: 3 before 2. Repeated: 1 again restarts, then 2 twice.
    assert_eq!(joined.push(&apps[2]).unwrap_err(), Error::UnexpectedFragment { expected: 2, found: 3 });
    assert_eq!(joined.expected(), Some(2), "kept");
    assert!(joined.push(&apps[1]).unwrap().is_none());
    assert_eq!(joined.push(&apps[1]).unwrap_err(), Error::UnexpectedFragment { expected: 3, found: 2 });
    assert!(joined.push(&apps[2]).unwrap().is_some(), "the request repeated, the message completes");
    assert_eq!(joined.expected(), None);
}

#[test]
fn a_fragment_from_another_meter_or_a_whole_telegram_is_refused() {
    // 004-compact-and-fragments/SR-003
    let mut bufs = [[0u8; 128]; 3];
    let apps = n3_apps(&mut bufs);
    let mut other_raw = hex(N3[1]);
    other_raw[4] ^= 0x01; // another identification number
    let crc = mbus_kit::crc::crc16(&other_raw[..10]).to_be_bytes();
    other_raw[10..12].copy_from_slice(&crc);
    let mut buf = [0u8; 128];
    let other = wireless::decode(&other_raw, Format::A, &mut buf).unwrap().unwrap().0.application().unwrap();
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    joined.push(&apps[0]).unwrap();
    assert_eq!(joined.push(&other).unwrap_err(), Error::FragmentFromOtherMeter);
    let whole = Application::parse(0x78, &[0x01, 0xFD, 0x08, 0x00]).unwrap();
    assert_eq!(joined.push(&whole).unwrap_err(), Error::UnexpectedFragment { expected: 2, found: 0 });
}

#[test]
fn a_message_longer_than_the_buffer_or_its_announced_length_is_an_error() {
    // 004-compact-and-fragments/SR-003, 004-compact-and-fragments/SR-005
    let mut bufs = [[0u8; 128]; 3];
    let apps = n3_apps(&mut bufs);
    let mut small = [0u8; 40];
    assert_eq!(Reassembly::new(&mut small).push(&apps[0]).unwrap_err(), Error::OutputTooSmall, "ML 86 > 40");
    // Fragments that do not add up to ML: drop the middle one by restarting the count.
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    let short = Application::parse(0x90, &[0x05, 0x01, 0x70, 0x40, 0x10, 0x00, 0x78, 0x01]).unwrap();
    joined.push(&short).unwrap();
    let last = Application::parse(0x90, &[0x02, 0x02, 0x00, 0xFD]).unwrap();
    assert_eq!(joined.push(&last).unwrap_err(), Error::InvalidLength, "ML 16, 3 bytes");
}

/// Rebuild the N.3 fragments with their pieces and MAC, push them, open the result.
fn join_n3(
    link: Option<mbus_kit::Address>,
    pieces: &[Vec<u8>],
    mac: &[u8],
) -> Option<Result<Authenticity, Error>> {
    let afl: [&[u8]; 3] = [
        &[0x09, 0x01, 0x78, 0x65, 0xB3, 0x0A, 0x00, 0x00, 0x56, 0x00],
        &[0x02, 0x02, 0x40],
        &[0x0A, 0x03, 0x04],
    ];
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    let key = Key::from_hex(KEY).unwrap();
    for (i, piece) in pieces.iter().enumerate() {
        let f = [afl[i], if i == 2 { mac } else { &[] }, piece].concat();
        let pushed = Application::parse_with_link(0x90, &f, link)
            .and_then(|a| joined.push(&a).map(|m| m.map(|m| m.body)));
        match pushed {
            Ok(None) => {}
            Ok(Some(Body::Encrypted(e))) => {
                let mut plain = [0u8; 256];
                return Some(e.decrypt(&key, &mut plain).map(|a| a.authenticity));
            }
            Ok(Some(_)) => return Some(Ok(Authenticity::None)),
            Err(e) => return Some(Err(e)),
        }
    }
    None
}

#[test]
fn every_flipped_bit_of_a_reassembled_mode_7_message_is_rejected() {
    // 004-compact-and-fragments/SR-002 (SC-002): every bit of the three pieces of the N.3 message and of its MAC.
    let mut bufs = [[0u8; 128]; 3];
    let apps = n3_apps(&mut bufs);
    let pieces: Vec<Vec<u8>> = apps
        .iter()
        .map(|a| match a.body {
            Body::Fragment(d) => d.to_vec(),
            _ => panic!(),
        })
        .collect();
    let mac = apps[2].afl.unwrap().mac().to_vec();
    let link = apps[0].meter();
    assert_eq!(join_n3(link, &pieces, &mac), Some(Ok(Authenticity::Authenticated { counter: 2739 })));
    let bits = (pieces.iter().map(Vec::len).sum::<usize>() + mac.len()) * 8;
    for bit in 0..bits {
        let (mut p, mut m) = (pieces.clone(), mac.clone());
        let mut k = bit / 8;
        match p.iter_mut().find(|x| {
            k < x.len() || {
                k -= x.len();
                false
            }
        }) {
            Some(x) => x[k] ^= 1 << (bit % 8),
            None => m[k] ^= 1 << (bit % 8),
        }
        let outcome = join_n3(link, &p, &m);
        assert!(matches!(outcome, Some(Err(_)) | Some(Ok(Authenticity::None))), "bit {bit}: {outcome:?}");
    }
}

#[test]
fn a_last_fragment_without_its_announced_mac_is_refused() {
    // Independent review 2026-11-27: fragment 1's MCL 65 announces an 8-byte CMAC; a last
    // fragment that drops it (FCL 03 00) must not open as Encrypted.
    let mut bufs = [[0u8; 128]; 3];
    let apps = n3_apps(&mut bufs);
    let pieces: Vec<Vec<u8>> = apps
        .iter()
        .map(|a| match a.body {
            Body::Fragment(d) => d.to_vec(),
            _ => panic!(),
        })
        .collect();
    let link = apps[0].meter();
    let frames = [
        [&[0x09u8, 0x01, 0x78, 0x65, 0xB3, 0x0A, 0x00, 0x00, 0x56, 0x00][..], &pieces[0]].concat(),
        [&[0x02u8, 0x02, 0x40][..], &pieces[1]].concat(),
        [&[0x02u8, 0x03, 0x00][..], &pieces[2]].concat(),
    ];
    let mut msg = [0u8; 256];
    let mut joined = Reassembly::new(&mut msg);
    let mut result = None;
    for f in &frames {
        if let Some(m) = joined.push(&Application::parse_with_link(0x90, f, link).unwrap()).unwrap() {
            let Body::Encrypted(e) = m.body else { panic!() };
            let mut plain = [0u8; 256];
            result = Some(e.decrypt(&Key::from_hex(KEY).unwrap(), &mut plain).map(|a| a.authenticity));
        }
    }
    assert_eq!(result, Some(Err(Error::Verification(mbus_kit::Check::Mac))));
}
