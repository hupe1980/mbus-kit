//! Compact frames (spec 004 US1) against published vectors: wmbusmeters `test_compact_crc`
//! and kamwater driver tests (GPL-3.0; bytes and expected values only), and the EN
//! 13757-3:2018 G.5 example as transcribed by exmbus (Apache-2.0).

use mbus_kit::compact::{Format, FormatCache, Formats};
use mbus_kit::wireless::{self, Format as Link};
use mbus_kit::{Application, Body, Check, Error};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn compact_of<'a>(app: &Application<'a>) -> mbus_kit::compact::Compact<'a> {
    match app.body {
        Body::Compact(c) => c,
        ref other => panic!("{other:?}"),
    }
}

/// wmbusmeters stores these telegrams already decrypted but with the ELL session number
/// still announcing AES-CTR; clear its encryption bits (31–29) so they parse as plaintext.
fn plain_ell(mut t: Vec<u8>) -> Vec<u8> {
    t[16] &= 0x1F;
    t
}

/// Learn the layout from a full frame, expand the compact one, render the records.
fn pair(full: &str, compact: &str) -> Vec<String> {
    let (full, compact) = (plain_ell(hex(full)), plain_ell(hex(compact)));
    let mut a = [0u8; 256];
    let (f, _) = wireless::decode(&full, Link::Stripped, &mut a).unwrap().unwrap();
    let app = f.application().unwrap();
    let mut layout = [0u8; 64];
    let format = Format::of(&app.records().unwrap(), &mut layout).unwrap();
    let mut cache: FormatCache<4, 64> = FormatCache::new();
    assert!(cache.insert(&format));
    let mut b = [0u8; 256];
    let (c, _) = wireless::decode(&compact, Link::Stripped, &mut b).unwrap().unwrap();
    let capp = c.application().unwrap();
    let compact = compact_of(&capp);
    assert_eq!(compact.signature, format.signature);
    let mut out = [0u8; 256];
    compact.expand(&cache, &mut out).unwrap().map(|r| r.unwrap().to_string()).collect()
}

#[test]
fn format_signature_of_the_wmbusmeters_layout() {
    let layout = hex("04FF2304134413426C023B92013BA2013B06FF1BA1015B91015BA10167");
    assert_eq!(Format::received(0xA9F3, &layout).unwrap().signature, 0xA9F3);
    assert!(Format::received(0xA9F4, &layout).is_none());
}

#[test]
fn kamstrup_compact_frame_in_format_b_expands_and_its_crc_catches_corruption() {
    let layout = hex("04FF2304134413426C023B92013BA2013B06FF1BA1015B91015BA10167");
    let format = Format::received(0xA9F3, &layout).unwrap();
    let good = hex(
        "3644372c235708273c068d20af00000000098079f3a9a73b000000000709000018080000413b00002e000000ffffffffffff1f2616bf70",
    );
    let mut buf = [0u8; 128];
    let (f, _) = wireless::decode(&good, Link::B, &mut buf).unwrap().unwrap();
    let app = f.application().unwrap();
    let mut out = [0u8; 128];
    let records: Vec<String> =
        compact_of(&app).expand(&format, &mut out).unwrap().map(|r| r.unwrap().to_string()).collect();
    assert_eq!(records[1], "volume — 2.311 m³", "04 13 = 0x0907 litres");
    // 004-compact-and-fragments/SR-001: the corrupted twin passes both link CRCs; only the
    // full-frame CRC catches it.
    let bad = hex(
        "3644372c235708273c068d20af00000000098079f3a9a73b000000000609000018080000413b00002e000000fffffffdffff1f2616bf70",
    );
    let mut buf = [0u8; 128];
    let (f, _) = wireless::decode(&bad, Link::B, &mut buf).unwrap().unwrap();
    let app = f.application().unwrap();
    let mut out = [0u8; 128];
    assert_eq!(
        compact_of(&app).expand(&format, &mut out).unwrap_err(),
        Error::Verification(Check::FullFrameCrc)
    );
}

#[test]
fn kamwater_full_and_compact_pairs_decode_the_same_values() {
    // MyTapWater 76348799: total_m3 6.408, target_m3 6.408.
    let r = pair(
        "2A442D2C998734761B168D2091D37CAC21576C7802FF207100041308190000441308190000615B7F616713",
        "23442D2C998734761B168D2087D19EAD217F1779EDA86AB6710008190000081900007F13",
    );
    assert_eq!(r[1], "volume — 6.408 m³");
    assert_eq!(r[2], "volume · storage 1 — 6.408 m³");
    // Vadden 44556677: total_m3 20.015, max_flow_last_day_m3h 0.317.
    let r = pair(
        "2D442D2C776655441B168D2083B48D3A2046887802FF20000004132F4E000092013B3D01A1015B028101E7FF0F03",
        "21442D2C776655441B168D2079CC8C3A20F4307912C40DFF00002F4E00003D010203",
    );
    assert_eq!(r[1], "volume — 20.015 m³");
    assert_eq!(r[2], "volume flow · maximum · storage 2 — 0.317 m³/h");
    // Votten 23813076: total_m3 3.871, target_date 2022-11-01; the compact frame's flow 0.035.
    let r = pair(
        "4744372C763081233C168D2056D1ED11205A3C7804FF2300080000441300000000523B000006FF1B08900008F0FF426CC12B61670A51671B023B000004131F0F00008101E7FF0F13",
        "3244372C763081233C168D2057D2ED112005387905095D330008000000000000000008900008F0FFC12B0A1B23001F0F000013",
    );
    assert!(r.contains(&"volume — 3.871 m³".to_owned()), "{r:?}");
    assert!(r.contains(&"date · storage 1 — 2022-11-01".to_owned()), "{r:?}");
    assert!(r.contains(&"volume flow — 0.035 m³/h".to_owned()), "{r:?}");
}

#[test]
fn en_13757_3_g5_example_format_and_compact_frames() {
    // Format frame 69 08 313B 02020215022A, compact frame 79 313B 42A6 D204 2E16 3423.
    let bytes1 = hex("08313B02020215022A");
    let app = Application::parse(0x69, &bytes1).unwrap();
    let Body::Format(format) = app.body else { panic!() };
    assert_eq!(format.signature, 0x3B31);
    let bytes2 = hex("313B42A6D2042E163423");
    let app = Application::parse(0x79, &bytes2).unwrap();
    let mut out = [0u8; 32];
    let r: Vec<String> =
        compact_of(&app).expand(&format, &mut out).unwrap().map(|r| r.unwrap().to_string()).collect();
    assert_eq!(r, ["energy — 123.4 Wh", "volume — 567.8 m³", "power — 901.2 W"]);
    // The full frame has the same layout.
    let bytes3 = hex("0202D20402152E16022A3423");
    let full = Application::parse(0x78, &bytes3).unwrap();
    let mut layout = [0u8; 16];
    assert_eq!(Format::of(&full.records().unwrap(), &mut layout).unwrap(), format);
}

#[test]
fn a_layout_that_cannot_describe_values_is_refused() {
    // 004-compact-and-fragments/SR-001: manufacturer data (0F) or a plain-text unit (FC) in a
    // layout received over the air.
    let mut out = [0u8; 32];
    for layout in [&[0x02u8, 0x13, 0x0F][..], &[0x02, 0xFC]] {
        let format = Format::received(mbus_kit::crc::crc16(layout), layout).unwrap();
        let mut frame = format.signature.to_le_bytes().to_vec();
        frame.extend([0, 0, 1, 2, 3]);
        let app = Application::parse(0x79, &frame).unwrap();
        assert_eq!(
            compact_of(&app).expand(&format, &mut out).unwrap_err(),
            Error::InvalidFormat,
            "{layout:02X?}"
        );
    }
}

#[test]
fn unknown_layout_is_named_not_guessed() {
    // 004-compact-and-fragments/SR-001: nothing is guessed.
    let bytes4 = hex("313B42A6D2042E163423");
    let app = Application::parse(0x79, &bytes4).unwrap();
    let empty: FormatCache<2, 16> = FormatCache::new();
    let mut out = [0u8; 32];
    assert_eq!(compact_of(&app).expand(&empty, &mut out).unwrap_err(), Error::UnknownFormat(0x3B31));
    assert!(empty.headers(0x3B31).is_none());
}

#[test]
fn the_crc_belongs_to_the_compact_data_not_the_cached_frame() {
    // exmbus regression: the compact frame carries other readings than the full frame.
    let bytes5 = hex("0306EB24004306E723000314285E00426CBF2C022D030001FF2100");
    let full = Application::parse(0x78, &bytes5).unwrap();
    let mut layout = [0u8; 32];
    let format = Format::of(&full.records().unwrap(), &mut layout).unwrap();
    assert_eq!(format.signature, 0xF1E7);
    let bytes6 = hex("E7F1A3FCED2400E723002E5E00BF2C0D0000");
    let app = Application::parse(0x79, &bytes6).unwrap();
    let mut out = [0u8; 64];
    assert!(compact_of(&app).expand(&format, &mut out).is_ok());
}

#[test]
fn the_cache_replaces_the_oldest() {
    let a = [0x02u8, 0x02];
    let b = [0x02u8, 0x15];
    let c = [0x02u8, 0x2A];
    let mut cache: FormatCache<2, 8> = FormatCache::new();
    for h in [&a[..], &b[..], &c[..]] {
        assert!(cache.insert(&Format::received(mbus_kit::crc::crc16(h), h).unwrap()));
    }
    assert!(cache.headers(mbus_kit::crc::crc16(&a)).is_none());
    assert!(cache.headers(mbus_kit::crc::crc16(&c)).is_some());
    assert!(
        !cache.insert(&Format::received(mbus_kit::crc::crc16(&[0u8; 9]), &[0u8; 9]).unwrap()),
        "too long"
    );
}
