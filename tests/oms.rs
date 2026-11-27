//! OMS security against published vectors (spec 002). OMS Vol. 2 Annex N Release E
//! (frames as printed, format A with CRCs); wmbusmeters and jMBus vectors with their keys
//! (test data only, attributed in `tests/corpus/README.md`).
#![cfg(feature = "oms")]

use mbus_kit::app::{Authenticity, Security};
use mbus_kit::oms::Key;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Address, Body, Check, Error, Value};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

const N21: &str = "2E44931578563412330333637A2A0020255923C95AAA26D1B2E7493BC2AD013EC4A6F6D3529B520EDFF0EA6DEFC955B29D6D69EBF3EC8A";
const N23: &str = "434493157856341233037AC98C2075900F002C25B30A000021924D4FBA372FB66E017A75002007109058475F4BC9D1281DF878B80A1B0F98B629024AAC7279429398BFC549233C0140829B93BAA1";
const N23_STRIPPED: &str = "434493157856341233038C2075900F002C25B30A000021924D4F2FB66E017A75002007109058475F4BC91DF878B80A1B0F98B629024AAC727942BFC549233C0140829B93";
const MULTICAL21: &str =
    "2A442D2C998734761B168D2091D37CAC21E1D68CDAFFCD3DC452BD802913FF7B1706CA9E355D6C2701CC24";
const K_N21: &str = "0102030405060708090A0B0C0D0E0F11";
const K_N23: &str = "000102030405060708090A0B0C0D0E0F";
const K_MULTICAL21: &str = "28F64A24988064A079AA2C807D6102AE";

fn key(s: &str) -> Key {
    Key::from_hex(s).unwrap()
}

/// The first records' values as display strings.
fn values(app: &mbus_kit::Application<'_>) -> Vec<String> {
    app.records().unwrap().map(|r| r.unwrap().to_string()).collect()
}

#[test]
fn mode_5_short_header_annex_n_2_1() {
    let raw = hex(N21);
    let mut buf = [0u8; 512];
    let (t, used) = wireless::open(&raw, Format::A, &key(K_N21), &mut buf).unwrap().unwrap();
    assert_eq!(used, raw.len());
    assert_eq!(t.address.id_number(), Some(12_345_678));
    // 002-oms-security/SR-001
    assert_eq!(t.app.authenticity, Authenticity::Encrypted, "mode 5 is not authentication");
    assert_eq!(
        values(&t.app),
        ["volume — 28504.27 m³", "date and time — 2008-05-31 23:50", "error flags — 0"]
    );
}

#[test]
fn mode_7_with_afl_mac_is_authenticated_annex_n_2_3() {
    for (raw, format) in [(hex(N23), Format::A), (hex(N23_STRIPPED), Format::Stripped)] {
        let mut buf = [0u8; 512];
        let (t, _) = wireless::open(&raw, format, &key(K_N23), &mut buf).unwrap().unwrap();
        // 002-oms-security/SR-001, SR-005: Authenticated only after the CMAC check, with its counter.
        assert_eq!(t.app.authenticity, Authenticity::Authenticated { counter: 2739 });
        assert_eq!(t.app.afl.unwrap().counter, Some(2739));
        assert_eq!(t.app.ell.unwrap().access_number, 0x75);
        assert_eq!(values(&t.app)[0], "volume — 28504.27 m³");
    }
}

#[test]
fn mode_7_wired_annex_n_2_5() {
    let raw = hex(
        "683131680803900F002C25B30A0000A08518CCB022C5FD7278563412931533037500100710D371C801D409B0D928D5659759C2EC935B16",
    );
    let (mbus_kit::wired::Frame::Long { ci, data, .. }, _) =
        mbus_kit::wired::Frame::parse(&raw).unwrap().unwrap()
    else {
        panic!()
    };
    let app = mbus_kit::Application::parse(ci, data).unwrap();
    let Body::Encrypted(e) = &app.body else { panic!() };
    assert_eq!(e.security, Security::Mode7);
    let mut out = [0u8; 64];
    let inner = e.decrypt(&key(K_N23), &mut out).unwrap();
    assert_eq!(inner.authenticity, Authenticity::Authenticated { counter: 2739 });
    assert_eq!(values(&inner), ["volume — 28504.27 m³", "date and time — 2008-05-31 23:50"]);
}

#[test]
fn mode_5_long_header_uses_the_meter_address_annex_n_6_4() {
    // Wired, partial encryption (1 block), plain tail after it; IV from the TPL address.
    let raw = hex(
        "682C2C6808FD7288776655934455080004100500DFE2A782146D1513581CD2F83F39040CFD10785634120C78443322112616",
    );
    let (mbus_kit::wired::Frame::Long { ci, data, .. }, _) =
        mbus_kit::wired::Frame::parse(&raw).unwrap().unwrap()
    else {
        panic!()
    };
    let app = mbus_kit::Application::parse(ci, data).unwrap();
    let Body::Encrypted(e) = &app.body else { panic!() };
    assert_eq!(e.encrypted_len, 16);
    let mut out = [0u8; 64];
    let inner = e.decrypt(&key(K_N23), &mut out).unwrap();
    let v = values(&inner);
    assert_eq!(v[0], "heat cost allocation — 1234");
    assert_eq!(v[1], "date · storage 1 — 2007-04-30");
    assert_eq!(v[4], "fabrication number — 11223344", "plain tail after the encrypted block");
}

#[test]
fn ell_aes_ctr_kamstrup_multical_21() {
    let raw = hex(MULTICAL21);
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&raw, Format::Stripped, &key(K_MULTICAL21), &mut buf).unwrap().unwrap();
    assert_eq!(t.app.authenticity, Authenticity::Encrypted);
    let volume = t.app.records().unwrap().nth(1).unwrap().unwrap();
    assert_eq!(volume.quantity().unwrap().to_string(), "6.408 m³", "wmbusmeters: total_m3 6.408");
}

#[test]
fn ell_aes_ctr_jmbus_vector() {
    // jMBus DecryptionTest: the byte after L (89) is outside the frame.
    let raw = hex("24442D2C692845631B168D3050209CD621B006B1140AEF4953AE5B86FAFC0B00E70705B84689");
    let mut buf = [0u8; 512];
    let k = key("4E5508544202058100DFEFA06B0934A5");
    let (t, used) = wireless::open(&raw, Format::Stripped, &k, &mut buf).unwrap().unwrap();
    assert_eq!(used, raw.len() - 1);
    let r = t.app.records().unwrap().nth(1).unwrap().unwrap();
    assert_eq!(r.quantity().unwrap().to_string(), "474.240 m³", "jMBus: 474.24");
    // jMBus's wrong-key test.
    let wrong = key("4E5508544202058100DFEFA06B0934AF");
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&raw, Format::Stripped, &wrong, &mut buf).unwrap_err(),
        Error::Verification(Check::PayloadCrc)
    );
}

#[test]
fn type_i_date_from_a_real_meter() {
    let raw = hex(
        "31444514E301000001028C20807A2E002025E25DEA4F2BC8EC94920260443A95C2BF3856E3E380E889CC8233CA5D9771F289",
    );
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&raw, Format::Stripped, &key("FCC9B808216D9D1F5FF6E1A3CF7D3084"), &mut buf)
        .unwrap()
        .unwrap();
    let r = t.app.records().unwrap().next().unwrap().unwrap();
    let Value::DateTime(dt) = r.value else { panic!("{:?}", r.value) };
    assert_eq!(dt.to_string(), "2000-02-01 13:54:50", "wmbusmeters ebeco500: meter_datetime");
}

// ---- 002-oms-security/SR-002: failed checks reject, never return records

#[test]
fn wrong_key_fails_the_named_check() {
    let mut buf = [0u8; 512];
    let wrong = key("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF");
    assert_eq!(
        wireless::open(&hex(N21), Format::A, &wrong, &mut buf).unwrap_err(),
        Error::Verification(Check::Marker)
    );
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&hex(N23), Format::A, &wrong, &mut buf).unwrap_err(),
        Error::Verification(Check::Mac)
    );
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&hex(MULTICAL21), Format::Stripped, &wrong, &mut buf).unwrap_err(),
        Error::Verification(Check::PayloadCrc)
    );
}

#[test]
fn every_flipped_bit_after_the_link_layer_is_rejected() {
    // SC-002: mode 7 (MAC) and ELL (payload CRC); stripped frames so the link CRC does
    // not catch it first. Bits in the address change the IV/KDF and fail as well.
    for (frame, k, start) in [(N23_STRIPPED, K_N23, 10), (MULTICAL21, K_MULTICAL21, 13)] {
        let raw = hex(frame);
        for i in start..raw.len() {
            for bit in 0..8 {
                let mut bad = raw.clone();
                bad[i] ^= 1 << bit;
                let mut buf = [0u8; 512];
                let r = wireless::open(&bad, Format::Stripped, &key(k), &mut buf);
                let accepted = matches!(&r, Ok(Some((t, _))) if t.app.records().is_some_and(|mut r| r.all(|x| x.is_ok())));
                assert!(
                    !accepted || is_unauthenticated_field(frame, i),
                    "{frame}: byte {i} bit {bit} accepted: {r:?}"
                );
            }
        }
    }
}

/// Bytes a MAC or payload CRC does not cover by design: the ELL CC bits H and R (cleared
/// for the IV), and in N.2.3 the ELL and the AFL header bytes before the MAC input.
fn is_unauthenticated_field(frame: &str, i: usize) -> bool {
    if frame == N23_STRIPPED {
        // L C M A (0..10) ELL 8C CC ACC (10..13), AFL 90 AFLL FCL (13..17); MCL and MCR
        // are MAC input; the MAC itself (22..30) only changes the comparison.
        (10..17).contains(&i)
    } else {
        // ELL CI, CC (H, R bits), ACC: 10, 11, 12.
        (10..13).contains(&i)
    }
}

#[test]
fn missing_key_leaves_the_telegram_encrypted() {
    let other = Address {
        id: [0, 0, 0, 0],
        ..wireless::decode(&hex(N21), Format::A, &mut [0; 128]).unwrap().unwrap().0.address
    };
    let keys = [(other, key(K_N21))];
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&hex(N21), Format::A, &keys, &mut buf).unwrap().unwrap();
    assert!(matches!(t.app.body, Body::Encrypted(_)));
    assert_eq!(t.app.authenticity, Authenticity::None);
}

#[test]
fn a_key_store_picks_the_meter_key() {
    let raw = hex(N21);
    let meter = wireless::decode(&raw, Format::A, &mut [0; 128]).unwrap().unwrap().0.address;
    let keys = [(Address { id: [1, 2, 3, 4], ..meter }, key(K_N23)), (meter, key(K_N21))];
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&raw, Format::A, &keys, &mut buf).unwrap().unwrap();
    assert_eq!(t.app.authenticity, Authenticity::Encrypted);
}

// ---- 002-oms-security/SR-003: keys never appear in output

#[test]
fn keys_and_errors_never_show_key_material() {
    let k = key(K_N21);
    let debug = format!("{k:?}");
    assert_eq!(debug, "Key(..)");
    for e in [Error::Verification(Check::Marker), Error::Verification(Check::Mac), Error::NoKey] {
        let text = format!("{e} {e:?}");
        assert!(!text.to_ascii_uppercase().contains("0102030405"), "{text}");
    }
}

// 002-oms-security/SR-004
#[test]
fn output_buffer_too_small_is_an_error_not_a_partial_result() {
    let raw = hex(N21);
    let mut frame = [0u8; 128];
    let (f, _) = wireless::decode(&raw, Format::A, &mut frame).unwrap().unwrap();
    let app = f.application().unwrap();
    let Body::Encrypted(e) = &app.body else { panic!() };
    let mut small = [0u8; 8];
    assert_eq!(e.decrypt(&key(K_N21), &mut small).unwrap_err(), Error::OutputTooSmall);
}

// ---- 002-oms-security/SR-007: decryption is not integrity, and the API says so
//
// These forgeries need no key. They succeed by design of OMS mode 5 and the ELL, which is
// why the result is `Encrypted`, never `Authenticated`.

fn crc16(d: &[u8]) -> u16 {
    mbus_kit::crc::crc16(d)
}

#[test]
fn ell_ctr_is_malleable_and_never_authenticated() {
    let raw = hex(MULTICAL21);
    // Stripped frame: L C M(2) A(6) | 8D CC ACC SN(4) | CRC(2) payload…
    let n = raw.len() - 19;
    let mut delta = vec![0u8; n];
    delta[n - 1] = 0x01;
    let dc = crc16(&delta) ^ crc16(&vec![0u8; n]);
    let mut forged = raw.clone();
    forged[19 + n - 1] ^= 0x01;
    forged[17] ^= dc.to_le_bytes()[0];
    forged[18] ^= dc.to_le_bytes()[1];
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&forged, Format::Stripped, &key(K_MULTICAL21), &mut buf).unwrap().unwrap();
    assert_eq!(t.app.authenticity, Authenticity::Encrypted, "a forgery must never look authenticated");
    let last = t.app.records().unwrap().last().unwrap().unwrap();
    assert_eq!(
        last.to_string(),
        "external temperature · minimum · storage 1 — 18 °C",
        "19 °C changed without the key"
    );
}

#[test]
fn mode_5_is_malleable_through_the_iv_and_never_authenticated() {
    let mut frame = [0u8; 128];
    let len = {
        let (f, _) = wireless::decode(&hex(N21), Format::A, &mut frame).unwrap().unwrap();
        f.payload.len() + 11
    };
    let mut stripped = frame[..len].to_vec();
    stripped[8] ^= 0x01; // link-layer version: part of the IV, not of the key lookup
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&stripped, Format::Stripped, &key(K_N21), &mut buf).unwrap().unwrap();
    assert_eq!(t.app.authenticity, Authenticity::Encrypted);
    assert_eq!(values(&t.app)[0], "volume — 28404.27 m³", "28504.27 changed without the key");
}

#[test]
fn records_say_whether_they_came_out_of_the_ciphertext() {
    // Annex N N.6.4: one encrypted block, then a plain tail nothing protects.
    let raw = hex(
        "682C2C6808FD7288776655934455080004100500DFE2A782146D1513581CD2F83F39040CFD10785634120C78443322112616",
    );
    let (mbus_kit::wired::Frame::Long { ci, data, .. }, _) =
        mbus_kit::wired::Frame::parse(&raw).unwrap().unwrap()
    else {
        panic!()
    };
    let app = mbus_kit::Application::parse(ci, data).unwrap();
    let Body::Encrypted(e) = &app.body else { panic!() };
    let mut out = [0u8; 64];
    let inner = e.decrypt(&key(K_N23), &mut out).unwrap();
    let flags: Vec<bool> = inner.records().unwrap().map(|r| r.unwrap().from_ciphertext).collect();
    assert_eq!(flags, [true, true, true, false, false]);
    // Plaintext records are never "from ciphertext"; an ELL payload always is.
    let plain = mbus_kit::Application::parse(0x78, &[0x01, 0x13, 0x05]).unwrap();
    assert!(!plain.records().unwrap().next().unwrap().unwrap().from_ciphertext);
    let mut buf = [0u8; 512];
    let (t, _) =
        wireless::open(&hex(MULTICAL21), Format::Stripped, &key(K_MULTICAL21), &mut buf).unwrap().unwrap();
    assert!(t.app.records().unwrap().all(|r| r.unwrap().from_ciphertext));
}

// ---- spec 004 US2: mode 10 (AES-CCM)

const N26: &str = "30449315785634123303B56B8C20757A7500112A1001B30A0000C882C518038E223BBC1091CEB8C0A1009D388A5A5E96F7883A5B5A7FD684D8";
const KAMWATER10: &str = "4544372c060382533c168c20307a3010ff2a10018c570e006da7632471f42b0a3ceccd461353573ed02b87118bc5ffe982f7f00a0274543bc51938f8b9c8bb8928114d60788a";
const KAMWATER10_BAD: &str = "4544372c060382533c168c20307a3010ff2a1001dc2e0d00cf1a85672e138d6c0af61338403fdeb528ab264e2c61e41b06073550aa296ae9815c8a7a9868ec6c624cfbbcecb3";

#[test]
fn mode_10_annex_n_2_6_is_authenticated_by_its_tag() {
    // 004-compact-and-fragments/SR-004
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&hex(N26), Format::A, &key(K_N23), &mut buf).unwrap().unwrap();
    assert_eq!(t.app.authenticity, Authenticity::Authenticated { counter: 2739 });
    assert_eq!(t.app.header.short().unwrap().counter, Some(2739));
    assert_eq!(
        values(&t.app),
        ["volume — 28504.27 m³", "date and time — 2008-05-31 23:50", "error flags — 0"]
    );
}

#[test]
fn mode_10_real_kamstrup_and_its_corrupted_twin() {
    let k = key("BF902DD6DFADAFE83E4AE6832B5FA14C");
    let mut buf = [0u8; 512];
    let (t, _) = wireless::open(&hex(KAMWATER10), Format::Stripped, &k, &mut buf).unwrap().unwrap();
    assert!(matches!(t.app.authenticity, Authenticity::Authenticated { .. }));
    let v = values(&t.app);
    assert!(v.contains(&"volume — 0.0 m³".to_owned()), "wmbusmeters: total_m3 0 ({v:?})");
    // The twin's keystream still yields plausible records: only the tag tells.
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&hex(KAMWATER10_BAD), Format::Stripped, &k, &mut buf).unwrap_err(),
        Error::Verification(Check::Tag)
    );
}

#[test]
fn mode_10_rejects_every_flipped_bit_after_the_ell() {
    // 004-compact-and-fragments/SR-004 (SC-002): stripped N.2.6, every bit from the TPL CI on.
    let mut frame = [0u8; 128];
    let len = {
        let (f, _) = wireless::decode(&hex(N26), Format::A, &mut frame).unwrap().unwrap();
        f.payload.len() + 11
    };
    let raw = frame[..len].to_vec();
    for i in 13..raw.len() {
        for bit in 0..8 {
            let mut bad = raw.clone();
            bad[i] ^= 1 << bit;
            let mut buf = [0u8; 512];
            let r = wireless::open(&bad, Format::Stripped, &key(K_N23), &mut buf);
            assert!(
                !matches!(&r, Ok(Some((t, _))) if t.app.authenticity != Authenticity::None),
                "byte {i} bit {bit}: {r:?}"
            );
        }
    }
}

// ---- independent review 2026-11-27 (spec 006 phase "review fixes")

#[test]
fn mode_10_records_appended_outside_the_tag_are_refused() {
    // 004-compact-and-fragments/SR-004: N.2.6 stripped, `04 13 FF FF FF 00` inserted
    // between the 17 encrypted bytes and the tag, L raised by 6. Before the fix this came
    // out Authenticated with an extra record "volume — 16777.215 m³".
    let forged = hex(
        "364493157856341233038C20757A7500112A1001B30A0000C882038E223BBC1091CEB8C0A1009D388A0413FFFFFF005AF7883A5B5A7FD6",
    );
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&forged, Format::Stripped, &key(K_N23), &mut buf).unwrap_err(),
        Error::Verification(Check::Tag)
    );
}

#[test]
fn a_stripped_mode_7_mac_is_an_error_not_a_downgrade() {
    // 002-oms-security/SR-002: MCL 25 announces AT 5 (8-byte CMAC); FCL 00 28 drops MACP
    // and the MAC with it.
    let mut t = hex(N23_STRIPPED);
    // L, ELL (3), AFL: 90 0F | 00 2C 25 B30A0000 | MAC(8)
    let afl = 13; // index of CI 90
    assert_eq!(t[afl], 0x90);
    t[afl + 1] = 0x07;
    t[afl + 3] = 0x28;
    t.drain(afl + 9..afl + 17);
    t[0] -= 8;
    let mut buf = [0u8; 512];
    assert_eq!(
        wireless::open(&t, Format::Stripped, &key(K_N23), &mut buf).unwrap_err(),
        Error::Verification(Check::Mac)
    );
}

#[test]
fn mode_13_and_other_modes_stay_encrypted() {
    // CF 0D01: mode 13 with 1 in the low byte; must not become plaintext records.
    let bytes = hex("01000D0D011703030020");
    let app = mbus_kit::Application::parse(0x7A, &bytes).unwrap();
    let Body::Encrypted(e) = app.body else { panic!("{:?}", app.body) };
    assert_eq!(e.security, Security::Other(13));
}
