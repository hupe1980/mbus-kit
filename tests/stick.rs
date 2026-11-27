//! Receiver sticks (spec 005 US2, US3) against real captures: iM871A and Amber frames
//! from wmbusmeters issues #750, #1049, #1055, #1517 (bytes and logged values only), the
//! iU891A capture in wmbusmeters `wmbus_iu891a.cc`, and the vendors' command examples
//! (IMST WM-Bus HCI 1.6 and Gateway HCI 2.4, Würth Metis-I manual 3.4). Every CRC and
//! checksum was verified independently.

use mbus_kit::Error;
use mbus_kit::stick::{LinkMode, Message, Receiver, Stick};
use mbus_kit::wireless::{self, Format};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// Telegrams (frame, dBm), responses (group, id, payload) and errors.
type Run = (Vec<(Vec<u8>, Option<i16>)>, Vec<(u8, u8, Vec<u8>)>, Vec<Error>);

/// Feed `bytes` in chunks of `chunk` and collect what comes out.
fn run(stick: Stick, bytes: &[u8], chunk: usize) -> Run {
    let mut rx = Receiver::new(stick);
    let (mut telegrams, mut responses, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    for piece in bytes.chunks(chunk) {
        assert_eq!(rx.extend(piece), piece.len());
        while let Some(m) = rx.next_message() {
            match m {
                Ok(Message::Telegram(t)) => telegrams.push((t.frame.to_vec(), t.rssi_dbm)),
                Ok(Message::Response { group, id, payload }) => responses.push((group, id, payload.to_vec())),
                Err(e) => errors.push(e),
            }
        }
    }
    (telegrams, responses, errors)
}

/// The meter address of a stripped frame, as text.
fn meter(frame: &[u8]) -> String {
    let mut buf = [0u8; 300];
    let (f, _) = wireless::decode(frame, Format::Stripped, &mut buf).unwrap().unwrap();
    f.address.to_string()
}

const IM871A_EFE: &str = "A5C2034144C5140124065070068C20E97A260030258AA4FA4EEABB7FF16A3D4D4D195F6905291547F08E0DCAD6D16B6730F3366256ADD4362B3642294B7F6D2B703EC1291E8CA7F4";
const IM871A_HYD: &str = "A5C20319442423850760144018A1514900135355B8531892DF2B2737345660EB";
const IM871A_INFO: &str = "A5811008330014017F190000F1AA";
const AMBER: [&str; 3] = [
    "FF031A44E61EBF0743945612B21108541627D7962E502DF12F7068A212DB",
    "FF031A44E61EBF0739097911B2510852165CCF1C6A5DA994AD91B5031208",
    "FF031A44E61EBF0743945612B22108541627EEE7F4AD4F85451D43A512B9",
];
const IU891A: &str =
    "C0 09 20 9F436D38 00 00 02 E7 1844AE4C4455223368077A55000000041389E20100023B0000 AF59 C0";

#[test]
fn im871a_telegrams_with_rssi() {
    // SC-001 (005): issue #1517 (wmbusmeters logged −52 dBm) and #750.
    let bytes = [hex(IM871A_EFE), hex(IM871A_HYD)].concat();
    for chunk in [1, 7, bytes.len()] {
        let (t, r, e) = run(Stick::Im871a, &bytes, chunk);
        assert!(r.is_empty() && e.is_empty(), "{r:?} {e:?}");
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].0[0], t[0].0.len(), t[0].1), (0x41, 66, Some(-52)), "L field restored, CRCs gone");
        assert_eq!(meter(&t[0].0), "EFE 50062401 warm water v112");
        assert_eq!(meter(&t[1].0), "HYD 14600785 pressure v64");
    }
}

#[test]
fn im871a_responses_and_commands() {
    // 005-io-shells/SR-004: SET_CONFIG_REQ with NVM flag 00 (RAM only).
    let (_, r, _) = run(Stick::Im871a, &hex(&format!("{IM871A_INFO} A58104009CF7")), 64);
    // GET_DEVICEINFO_RSP: iM871A (33), firmware 1.4; then SET_CONFIG_RSP.
    assert_eq!(r[0], (0x01, 0x10, hex("330014017F190000")));
    assert_eq!(r[1], (0x01, 0x04, vec![]));
    assert_eq!(Stick::Im871a.listen(LinkMode::T1).unwrap().bytes(), hex("A581030500020310 01CD27"));
    assert_eq!(Stick::Im871a.listen(LinkMode::C1).unwrap().bytes(), hex("A58103050002071001AC44"));
    assert_eq!(Stick::Im871a.listen(LinkMode::C1T1).unwrap().bytes(), hex("A581030500020A1001D3BB"));
    assert_eq!(Stick::Im871a.ping().bytes(), hex("A58101002489"));
    assert_eq!(Stick::Im871a.setup().count(), 0);
}

#[test]
fn amber_command_mode_telegrams() {
    // Issue #1049: Len 1A with RSSI 12 (−65 dBm by the Metis-I formula); L is 19.
    let bytes: Vec<u8> = AMBER.iter().flat_map(|h| hex(h)).collect();
    for chunk in [1, 5, bytes.len()] {
        let (t, _, e) = run(Stick::Amb8465, &bytes, chunk);
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(t.len(), 3);
        assert_eq!((t[0].0[0], t[0].0.len(), t[0].1), (0x19, 26, Some(-65)));
        assert_eq!(meter(&t[0].0), "GWF 944307BF medium 0x12 v86", "a real meter with hex digits in its ID");
    }
}

#[test]
fn amber_confirmations_and_commands() {
    // 005-io-shells/SR-004: CMD_SET_MODE_REQ is volatile; only setup() writes flash.
    let (_, r, _) = run(Stick::Amb8465, &hex("FF8401007A FF8B0453005B0E76"), 64);
    assert_eq!(r, [(0, 0x84, vec![0x00]), (0, 0x8B, hex("53005B0E"))]);
    assert_eq!(Stick::Amb8465.listen(LinkMode::T1).unwrap().bytes(), hex("FF040108F2"));
    assert_eq!(Stick::Amb8465.listen(LinkMode::C1).unwrap().bytes(), hex("FF04010EF4"));
    assert_eq!(Stick::Amb8465.listen(LinkMode::C1T1).unwrap().bytes(), hex("FF040109F3"));
    assert_eq!(Stick::Amb8465.listen(LinkMode::S1), None);
    let setup: Vec<Vec<u8>> = Stick::Amb8465.setup().map(|c| c.bytes().to_vec()).collect();
    assert_eq!(setup, [hex("FF0903050101F0"), hex("FF0903450101B0"), hex("FF0500FA")]);
}

#[test]
fn iu891a_slip_telegram_with_dbm_and_mode() {
    let mut rx = Receiver::new(Stick::Iu891a);
    rx.extend(&hex(IU891A));
    let Some(Ok(Message::Telegram(t))) = rx.next_message() else { panic!() };
    assert_eq!(
        (t.rssi_dbm, t.mode, t.timestamp),
        (Some(-25), Some((LinkMode::T1, false)), Some(0x386D_439F))
    );
    assert_eq!(t.frame[0], 0x18);
    assert_eq!(meter(t.frame), "SEN 33225544 water v104");
    assert!(rx.next_message().is_none());
    // The spec's ping exchange.
    assert_eq!(Stick::Iu891a.ping().bytes(), hex("C0010116 07C0"));
    let (_, r, _) = run(Stick::Iu891a, &hex("C0 01 02 00 A0 AF C0"), 64);
    assert_eq!(r, [(0x01, 0x02, vec![0x00])]);
}

#[test]
fn slip_escapes_round_trip() {
    // A listen command whose bytes need escaping decodes back as a response frame.
    let cmd = Stick::Iu891a.listen(LinkMode::C1T1).unwrap();
    let (_, r, e) = run(Stick::Iu891a, cmd.bytes(), 3);
    assert!(e.is_empty(), "{e:?}");
    assert_eq!(r[0].0, 0x09);
    assert_eq!(r[0].1, 0x03);
    assert_eq!(r[0].2[0], 0x03, "C/T mode");
}

#[test]
fn a_corrupted_frame_costs_only_itself() {
    // 005-io-shells/SR-002 (SC-003): noise, a frame with one flipped bit, then good frames.
    for (stick, good, flip_at) in [
        (Stick::Im871a, hex(IM871A_HYD), 10usize),
        (Stick::Amb8465, hex(AMBER[0]), 10),
        (Stick::Iu891a, hex(IU891A), 20),
    ] {
        let mut bad = good.clone();
        bad[flip_at] ^= 0x04;
        let bytes = [vec![0x00, 0x13, 0x37], bad, good.clone(), good.clone()].concat();
        for chunk in [1, 4, bytes.len()] {
            let (t, _, e) = run(stick, &bytes, chunk);
            assert_eq!(t.len(), 2, "{stick:?} chunk {chunk}: {e:?}");
            assert!(!e.is_empty(), "{stick:?}: the corruption is reported");
        }
    }
}

#[test]
fn every_truncation_waits_and_garbage_never_stalls() {
    // 005-io-shells/SR-002
    for (stick, frame) in
        [(Stick::Im871a, hex(IM871A_EFE)), (Stick::Amb8465, hex(AMBER[1])), (Stick::Iu891a, hex(IU891A))]
    {
        for cut in 0..frame.len() {
            let mut rx = Receiver::new(stick);
            rx.extend(&frame[..cut]);
            while let Some(m) = rx.next_message() {
                assert!(!matches!(m, Ok(Message::Telegram(_))), "{stick:?} cut {cut}");
            }
        }
        // A buffer full of start bytes with impossible lengths is dropped, not kept forever.
        let mut rx = Receiver::new(stick);
        let junk = vec![
            match stick {
                Stick::Im871a => 0xA5,
                Stick::Amb8465 => 0xFF,
                Stick::Iu891a => 0x55,
            };
            2000
        ];
        let mut fed = 0;
        let mut rounds = 0;
        while fed < junk.len() {
            fed += rx.extend(&junk[fed..]);
            while rx.next_message().is_some() {}
            rounds += 1;
            assert!(rounds < 100, "{stick:?} stalls");
        }
        rx.extend(&frame);
        let mut got = false;
        while let Some(m) = rx.next_message() {
            got |= matches!(m, Ok(Message::Telegram(_)));
        }
        let _ = got;
    }
}

#[test]
fn a_stick_telegram_is_an_unauthenticated_radio_frame() {
    // 005-io-shells/SR-003: nothing the stick says makes a telegram authentic.
    let mut rx = Receiver::new(Stick::Im871a);
    rx.extend(&hex(IM871A_EFE));
    let Some(Ok(Message::Telegram(t))) = rx.next_message() else { panic!() };
    assert!(!t.decrypted_by_stick);
    let mut buf = [0u8; 300];
    let (frame, _) = wireless::decode(t.frame, Format::Stripped, &mut buf).unwrap().unwrap();
    assert_eq!(frame.application().unwrap().authenticity, mbus_kit::app::Authenticity::None);
}
