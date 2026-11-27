//! Dates and times (EN 13757-3:2018, Annex A): data types G, J, F and I.
//!
//! | Type | Bytes | Content |
//! |---|---|---|
//! | G | 2 | date |
//! | J | 3 | time of day with seconds |
//! | F | 4 | date, hour, minute; invalid and summer-time flags |
//! | I | 6 | type F plus seconds, day of week and week number |
//!
//! The meter's local time; no time zone is transmitted. A date the meter flags invalid,
//! or one that cannot exist (month 13), is still returned: [`Date::is_valid`] and
//! [`DateTime::invalid`] say so. Iteration never stops on a bad date.

use core::fmt;

/// A date, data type G.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Date {
    /// Year (EN 13757-3 Table A.5: a two-digit year up to 80 is 20xx, above is 19xx).
    pub year: u16,
    /// Month, 1–12 when valid.
    pub month: u8,
    /// Day, 1–31 when valid.
    pub day: u8,
}

impl Date {
    /// Decode type G: day in bits 0–4, month in bits 8–11, year in bits 5–7 and 12–15.
    pub(crate) fn from_type_g(b0: u8, b1: u8) -> Self {
        let year = ((b0 & 0xE0) >> 5) | ((b1 & 0xF0) >> 1);
        Self { year: century(year, 0), month: b1 & 0x0F, day: b0 & 0x1F }
    }

    /// The date `months` later (or earlier, when negative), for monthly profiles and
    /// billing dates. The last day of a month stays the last day: 2008-01-31 + 1 month is
    /// 2008-02-29, + 2 months 2008-03-31 (OMS Vol. 2 Table G.5). Other days are clamped to
    /// the month's length. `None` for an invalid date or a year outside 0–65535.
    pub fn add_months(self, months: i32) -> Option<Self> {
        if !self.is_valid() {
            return None;
        }
        let last = self.day >= days_in(self.year, self.month);
        let total = (i64::from(self.year) * 12 + i64::from(self.month) - 1).checked_add(i64::from(months))?;
        let year = u16::try_from(total.div_euclid(12)).ok()?;
        let month = u8::try_from(total.rem_euclid(12) + 1).ok()?;
        let len = days_in(year, month);
        let day = if last { len } else { self.day.min(len) };
        Some(Self { year, month, day })
    }

    /// Whether the date can exist: month 1–12, day 1 to the month's length.
    pub const fn is_valid(&self) -> bool {
        let leap = (self.year % 4 == 0 && self.year % 100 != 0) || self.year % 400 == 0;
        let days = match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return false,
        };
        self.day >= 1 && self.day <= days
    }
}

const fn days_in(year: u16, month: u8) -> u8 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

/// The full year for a 7-bit year field and the two "hundred year" bits of type F/I.
///
/// EN 13757-3:2018 Table A.5 (as reproduced by exmbus; libmbus agrees): with hundred-year
/// bits 0 and a year up to 80, the year is 2000 + year (the compatibility rule for meters
/// with a two-digit year); otherwise 1900 + 100 × hundred-year + year. OMS Annex N encodes
/// 2008 with hundred-year 1.
const fn century(year: u8, hundred: u8) -> u16 {
    let y = year as u16;
    if hundred == 0 && y <= 80 { 2000 + y } else { 1900 + 100 * hundred as u16 + y }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)?;
        if !self.is_valid() {
            f.write_str(" (invalid)")?;
        }
        Ok(())
    }
}

/// A time of day, data type J (and the time part of F and I).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Time {
    /// Hour, 0–23.
    pub hour: u8,
    /// Minute, 0–59.
    pub minute: u8,
    /// Second, 0–59 (0 for type F, which has no seconds).
    pub second: u8,
}

impl Time {
    /// Decode type J: seconds, minutes and hours in the low bits of three bytes.
    pub(crate) const fn from_type_j(b0: u8, b1: u8, b2: u8) -> Self {
        Self { second: b0 & 0x3F, minute: b1 & 0x3F, hour: b2 & 0x1F }
    }

    /// Whether every field is in range.
    pub const fn is_valid(&self) -> bool {
        self.hour < 24 && self.minute < 60 && self.second < 60
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

/// A date and time, data type F (minutes) or I (seconds). The meter's local time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools)] // the flags are the wire format's
pub struct DateTime {
    /// The date.
    pub date: Date,
    /// The time of day.
    pub time: Time,
    /// Type I: the time has seconds. Type F: `time.second` is 0 and not meaningful.
    pub has_seconds: bool,
    /// The meter flags the time as invalid (IV bit).
    pub invalid: bool,
    /// The meter flags summer (daylight saving) time: SU bit (type F), DST bit (type I).
    pub summer_time: bool,
    /// Type I: the meter flags the year as a leap year.
    pub leap_year: bool,
    /// Type I: day of week, 1 = Monday … 7 = Sunday, 0 = not given.
    pub weekday: u8,
    /// Type I: week of the year, 1–53, 0 = not given.
    pub week: u8,
}

impl DateTime {
    /// Decode type F (4 bytes).
    pub(crate) fn from_type_f(b: [u8; 4]) -> Self {
        let [b0, b1, b2, b3] = b;
        let mut date = Date::from_type_g(b2, b3);
        date.year = century(((b2 & 0xE0) >> 5) | ((b3 & 0xF0) >> 1), (b1 >> 5) & 0x03);
        Self {
            date,
            time: Time { hour: b1 & 0x1F, minute: b0 & 0x3F, second: 0 },
            has_seconds: false,
            invalid: b0 & 0x80 != 0,
            summer_time: b1 & 0x80 != 0,
            leap_year: false,
            weekday: 0,
            week: 0,
        }
    }

    /// Decode type I (6 bytes, EN 13757-3:2018 Table A.8 as read by exmbus and libmbus):
    /// byte 0 leap year (7), DST (6), second; byte 1 IV (7), DST sign (6), minute;
    /// byte 2 day of week (7–5), hour; bytes 3–4 as type G; byte 5 DST deviation (7–6),
    /// week number.
    pub(crate) fn from_type_i(b: [u8; 6]) -> Self {
        let [b0, b1, b2, b3, b4, b5] = b;
        let date = Date::from_type_g(b3, b4);
        Self {
            date,
            time: Time { hour: b2 & 0x1F, minute: b1 & 0x3F, second: b0 & 0x3F },
            has_seconds: true,
            invalid: b1 & 0x80 != 0,
            summer_time: b0 & 0x40 != 0,
            leap_year: b0 & 0x80 != 0,
            weekday: b2 >> 5,
            week: b5 & 0x3F,
        }
    }

    /// A valid local date and time (to set a meter's clock with
    /// [`master::set_time`](crate::master::set_time)). `None` if it cannot exist or lies
    /// outside 1981–2299, the years type F can carry.
    pub fn new(year: u16, month: u8, day: u8, hour: u8, minute: u8) -> Option<Self> {
        let t = Self {
            date: Date { year, month, day },
            time: Time { hour, minute, second: 0 },
            has_seconds: false,
            invalid: false,
            summer_time: false,
            leap_year: false,
            weekday: 0,
            week: 0,
        };
        (t.is_valid() && (1981..=2299).contains(&year)).then_some(t)
    }

    /// Encode as type F (4 bytes), e.g. to set a meter's clock. Years 2000–2080 use
    /// hundred-year bits 0 (what meters expect), others the hundred-year bits.
    pub fn to_type_f(&self) -> [u8; 4] {
        let (y, h) = match self.date.year {
            2000..=2080 => (self.date.year - 2000, 0),
            year => ((year.saturating_sub(1900)) % 100, (year.saturating_sub(1900) / 100).min(3)),
        };
        #[allow(clippy::cast_possible_truncation)] // y < 100, h ≤ 3
        let (y, h) = (y as u8, h as u8);
        let Time { hour, minute, .. } = self.time;
        [
            (minute & 0x3F) | if self.invalid { 0x80 } else { 0 },
            (hour & 0x1F) | (h << 5) | if self.summer_time { 0x80 } else { 0 },
            (self.date.day & 0x1F) | ((y & 0x07) << 5),
            (self.date.month & 0x0F) | ((y & 0x78) << 1),
        ]
    }

    /// Whether the meter marks the time valid and every field can exist.
    pub const fn is_valid(&self) -> bool {
        !self.invalid && self.date.is_valid() && self.time.is_valid()
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Date { year, month, day } = self.date;
        let Time { hour, minute, second } = self.time;
        write!(f, "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")?;
        if self.has_seconds {
            write!(f, ":{second:02}")?;
        }
        if self.summer_time {
            f.write_str(" (summer time)")?;
        }
        if !self.is_valid() {
            f.write_str(" (invalid)")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn type_g() {
        // 2026-11-27: year 26 = 0b001_1010 → b0 bits 7–5 = 010, b1 bits 7–4 = 0011.
        let d = Date::from_type_g(0b0101_1011, 0b0011_1011);
        assert_eq!(d.to_string(), "2026-11-27");
        assert!(d.is_valid());
        let bad = Date::from_type_g(0xFF, 0xFF);
        assert!(!bad.is_valid());
        assert!(bad.to_string().ends_with("(invalid)"));
    }

    #[test]
    fn type_f_flags_and_century() {
        let t = DateTime::from_type_f([30, 14, 0b0101_1011, 0b0011_1011]);
        assert_eq!(t.to_string(), "2026-11-27 14:30");
        let t = DateTime::from_type_f([0x1E | 0x80, 0x0E | 0x80, 0b0101_1011, 0b0011_1011]);
        assert!(t.invalid && t.summer_time);
        assert_eq!(t.to_string(), "2026-11-27 14:30 (summer time) (invalid)");
        // Year 99 without hundred-year bits is 1999; OMS Annex N N.2.1 encodes 2008 as 8
        // with hundred-year 1 (`04 6D 32 37 1F 15` = 2008-05-31 23:50).
        assert_eq!(century(99, 0), 1999);
        assert_eq!(century(80, 0), 2080);
        let t = DateTime::from_type_f([0x32, 0x37, 0x1F, 0x15]);
        assert_eq!(t.to_string(), "2008-05-31 23:50");
    }

    #[test]
    fn month_steps_keep_the_month_end() {
        let d = Date { year: 2008, month: 1, day: 31 };
        let steps: std::vec::Vec<std::string::String> =
            (1..=4).map(|k| d.add_months(k).unwrap().to_string()).collect();
        assert_eq!(steps, ["2008-02-29", "2008-03-31", "2008-04-30", "2008-05-31"]);
        assert_eq!(Date { year: 2008, month: 5, day: 31 }.add_months(-4).unwrap().to_string(), "2008-01-31");
        assert_eq!(Date { year: 2020, month: 11, day: 15 }.add_months(3).unwrap().to_string(), "2021-02-15");
        // A meter's invalid date, or a step past the representable years: no date.
        assert_eq!(Date { year: 2020, month: 13, day: 1 }.add_months(1), None);
        assert_eq!(d.add_months(i32::MAX), None);
        assert_eq!(d.add_months(i32::MIN), None);
        // Type F ends at 2299 (hundred-year bits 3): 2350 would encode as 2250.
        assert!(DateTime::new(2299, 12, 31, 23, 59).is_some());
        assert!(DateTime::new(2300, 1, 1, 0, 0).is_none());
        let t = DateTime::new(2250, 6, 1, 12, 0).unwrap();
        assert_eq!(DateTime::from_type_f(t.to_type_f()).date.year, 2250);
    }

    #[test]
    fn type_f_round_trips() {
        for bytes in [
            [0x32, 0x37, 0x1F, 0x15],
            [30, 14, 0b0101_1011, 0b0011_1011],
            [0x1E | 0x80, 0x0E | 0x80, 0x5B, 0x3B],
        ] {
            let t = DateTime::from_type_f(bytes);
            assert_eq!(DateTime::from_type_f(t.to_type_f()), t, "{bytes:02x?}");
        }
    }

    #[test]
    fn type_i_real_vector() {
        // wmbusmeters ebeco500: `06 6D B2 36 0D 01 02 00` = 2000-02-01 13:54:50, leap year.
        let t = DateTime::from_type_i([0xB2, 0x36, 0x0D, 0x01, 0x02, 0x00]);
        assert_eq!(t.to_string(), "2000-02-01 13:54:50");
        assert!(t.leap_year && !t.summer_time && !t.invalid);
    }

    #[test]
    fn type_j_and_i() {
        assert_eq!(Time::from_type_j(5, 4, 3).to_string(), "03:04:05");
        let t = DateTime::from_type_i([59, 30, 0x0E | (5 << 5), 0b0101_1011, 0b0011_1011, 48]);
        assert_eq!(t.to_string(), "2026-11-27 14:30:59");
        assert_eq!((t.weekday, t.week), (5, 48));
    }
}
