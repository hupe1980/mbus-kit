//! Compact profiles (spec 004 US4) against OMS Vol. 2 Annex G (Tables G.6, G.7) and a
//! real Techem heat cost allocator (wmbusmeters vector; the OMS rule, not wmbusmeters'
//! reading, reproduces the meter's own storage-1 value).

use mbus_kit::profile::{Increment, Spacing};
use mbus_kit::vif::Direction;
use mbus_kit::{Records, Unit, Value};

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

#[test]
fn oms_g6_forward_increments() {
    // Base 65 l at storage 8 (2008-01-31), then increments 144, 214, 332, 258.
    let bytes = hex("8C0413 65000000  82046C 1F11  8D04931F 0A 7AFE 4401 1402 3203 5802");
    let records: Vec<_> = Records::new(&bytes).map(Result::unwrap).collect();
    let base = records[0].value.as_integer().unwrap();
    let p = records[2].profile().unwrap();
    assert_eq!(
        (p.direction, p.increment, p.spacing),
        (Direction::Forward, Increment::Positive, Spacing::Months(1))
    );
    assert_eq!(p.len(), 4);
    let v: Vec<_> = p.values(base).collect();
    assert_eq!(v, [Some(209), Some(423), Some(755), Some(1013)]);
    // Element k is storage 8 + k, one month after the base time each, month ends kept.
    let Value::Date(t) = records[1].value else { panic!() };
    assert_eq!(t.to_string(), "2008-01-31");
    assert_eq!(t.add_months(1).unwrap().to_string(), "2008-02-29");
    let dates: Vec<_> = (0..4).map(|k| p.date(k, t).unwrap().to_string()).collect();
    assert_eq!(dates, ["2008-02-29", "2008-03-31", "2008-04-30", "2008-05-31"]);
    assert_eq!(p.offset(4), None, "out of range");
    assert_eq!(records[2].to_string(), "volume · compact profile · storage 8 — 4 increments, every month");
}

#[test]
fn oms_g7_inverse_increments() {
    // Base 1013 l at storage 8 (2008-05-31), going back: 258, 332, 214, 144.
    let bytes = hex("8C0413 13100000  82046C 1F15  8D049313 0A 7AFE 5802 3203 1402 4401");
    let records: Vec<_> = Records::new(&bytes).map(Result::unwrap).collect();
    let p = records[2].profile().unwrap();
    assert_eq!(p.direction, Direction::Inverse);
    let v: Vec<_> = p.values(records[0].value.as_integer().unwrap()).collect();
    assert_eq!(v, [Some(755), Some(423), Some(209), Some(65)]);
    assert_eq!(p.offset(0), Some(mbus_kit::profile::Offset::Months(-1)), "inverse: back in time");
}

#[cfg(feature = "oms")]
#[test]
fn techem_heat_cost_allocator_history() {
    use mbus_kit::oms::Key;
    use mbus_kit::wireless::{self, Format};
    let raw = hex(
        "4E4468507620541494087AAD004005089D86B62A329B3439873999738F82461ABDE3C7AC78692B363F3B41EB68607F9C9160F550769B065B6EA00A2E44346E29FF5DC5CB86283C69324AD33D137F6F",
    );
    let mut buf = [0u8; 512];
    let key = Key::from_hex("FCF41938F63432975B52505F547FCEDF").unwrap();
    let (t, _) = wireless::open(&raw, Format::Stripped, &key, &mut buf).unwrap().unwrap();
    let records: Vec<_> = t.app.records().unwrap().map(Result::unwrap).collect();
    let storage_1 = records[1].value.as_integer().unwrap(); // 25 at 2020-12-31
    let base = records
        .iter()
        .find(|r| r.dib.storage == 8 && r.profile().is_none() && r.value.as_integer().is_some())
        .unwrap();
    let p = records.iter().find_map(mbus_kit::Record::profile).unwrap();
    assert_eq!(p.spacing, Spacing::Months(1));
    let v: Vec<_> = p.values(base.value.as_integer().unwrap()).collect();
    assert_eq!(v.last().copied().flatten(), Some(storage_1), "the history ends at the storage-1 reading");
    assert_eq!(v.iter().filter(|x| **x == Some(0)).count(), 12);
}

#[test]
fn illegal_elements_and_absolute_profiles() {
    // Absolute, 1-byte signed elements every 15 seconds; -128 is the "illegal" preset.
    let bytes = hex("8D0493 1F 05 01 0F 05 80 07");
    let r = Records::new(&bytes).next().unwrap().unwrap();
    let p = r.profile().unwrap();
    assert_eq!(p.spacing, Spacing::Every(15, Unit::Seconds));
    assert_eq!(p.values(0).collect::<Vec<_>>(), [Some(5), None, Some(7)]);
}

#[cfg(feature = "oms")]
#[test]
fn oms_n13_inverse_profiles_match_the_annex_table() {
    // OMS Annex N N.13: base 1995 Wh (+A) and 305 Wh (−A) at reception − 3 s, then every
    // 8 s further back; the Annex lists all 46 readings of each.
    use mbus_kit::oms::Key;
    use mbus_kit::wireless::{self, Format};
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/corpus/wireless/oms-n13-mode7-profile.hex"
    ))
    .unwrap();
    let raw = hex(text.lines().find(|l| l.starts_with("D344")).unwrap());
    let mut buf = [0u8; 512];
    let key = Key::from_hex("000102030405060708090A0B0C0D0E0F").unwrap();
    let (t, _) = wireless::open(&raw, Format::A, &key, &mut buf).unwrap().unwrap();
    let records: Vec<_> = t.app.records().unwrap().map(Result::unwrap).collect();
    let import: Vec<_> =
        records[4].profile().unwrap().values(records[3].value.as_integer().unwrap()).collect();
    let export: Vec<_> =
        records[6].profile().unwrap().values(records[5].value.as_integer().unwrap()).collect();
    assert_eq!(&import[..6], [Some(1994), Some(1991), Some(1986), Some(1979), Some(1970), Some(1962)]);
    assert_eq!((import.len(), import[45]), (46, Some(1872)));
    // Every 8 s back from the base time (reception − 3 s): #46 is 3 + 368 s before reception.
    assert_eq!(records[4].profile().unwrap().offset(45), Some(mbus_kit::profile::Offset::Seconds(-368)));
    assert_eq!((export[9], export[10], export[45]), (Some(305), Some(304), Some(215)));
    assert_eq!(
        records[2].value,
        Value::Duration(mbus_kit::Quantity { mantissa: 3, exponent: 0, unit: Some(Unit::Seconds) })
    );
}
