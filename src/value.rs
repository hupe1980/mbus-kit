use core::fmt;

/// A record's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value<'a> {
    /// No data.
    None,
    /// A binary integer.
    Int(i64),
    /// A BCD number.
    Bcd(i64),
    /// A BCD field with digits outside 0–9 (meters use this to signal errors). The raw
    /// bytes, as transmitted.
    InvalidBcd(&'a [u8]),
    /// A 32-bit IEEE 754 real, as raw bits. Not converted: values in this crate are
    /// exact (D-004), so turning these bits into a number is the caller's decision.
    Real32(u32),
    /// A date (type G).
    Date(Date),
    /// A date and time (type F).
    DateTime(DateTime),
    /// A text (variable length, LVAR `00`–`BF`), as transmitted (M-Bus sends it reversed).
    Text(&'a [u8]),
    /// Binary data longer than 8 bytes.
    Bytes(&'a [u8]),
}

impl Value<'_> {
    /// The value as an integer, for binary integers and valid BCD.
    pub const fn as_integer(&self) -> Option<i128> {
        match *self {
            Self::Int(v) | Self::Bcd(v) => Some(v as i128),
            _ => None,
        }
    }

    pub(crate) const fn negate(self) -> Self {
        match self {
            Self::Bcd(v) => Self::Bcd(-v),
            other => other,
        }
    }
}

/// Little-endian two's complement of 1 to 8 bytes.
pub(crate) fn le_signed(bytes: &[u8]) -> i64 {
    let negative = bytes.last().is_some_and(|b| b & 0x80 != 0);
    let start: u64 = if negative { u64::MAX } else { 0 };
    let raw = bytes.iter().rev().fold(start, |acc, &b| (acc << 8) | u64::from(b));
    i64::from_le_bytes(raw.to_le_bytes())
}

/// Decode little-endian BCD. A most significant nibble of `F` marks a negative number
/// (EN 13757-3); any other nibble above 9 makes the field invalid.
pub(crate) fn bcd(bytes: &[u8]) -> Value<'_> {
    let mut v: i64 = 0;
    let mut negative = false;
    for (i, &b) in bytes.iter().enumerate().rev() {
        for (j, digit) in [b >> 4, b & 0x0F].into_iter().enumerate() {
            let most_significant = i + 1 == bytes.len() && j == 0;
            if digit > 9 {
                if most_significant && digit == 0xF {
                    negative = true;
                    continue;
                }
                return Value::InvalidBcd(bytes);
            }
            v = v * 10 + i64::from(digit);
        }
    }
    Value::Bcd(if negative { -v } else { v })
}

/// Four BCD bytes (LSB first) as a decimal number, e.g. an identification number.
pub(crate) fn bcd_u32(bytes: [u8; 4]) -> Option<u32> {
    match bcd(&bytes) {
        Value::Bcd(v) => u32::try_from(v).ok(),
        _ => None,
    }
}

/// A unit of a quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unit {
    /// Watt-hour.
    Wh,
    /// Joule.
    J,
    /// Cubic metre.
    M3,
    /// Kilogram.
    Kg,
    /// Watt.
    W,
    /// Joule per hour.
    JPerH,
    /// Cubic metre per hour.
    M3PerH,
    /// Cubic metre per minute.
    M3PerMin,
    /// Cubic metre per second.
    M3PerS,
    /// Kilogram per hour.
    KgPerH,
    /// Degree Celsius.
    Celsius,
    /// Kelvin.
    Kelvin,
    /// Bar.
    Bar,
    /// Second.
    Seconds,
    /// Minute.
    Minutes,
    /// Hour.
    Hours,
    /// Day.
    Days,
}

impl Unit {
    pub(crate) const fn duration(nn: u8) -> Self {
        match nn & 0x03 {
            0 => Self::Seconds,
            1 => Self::Minutes,
            2 => Self::Hours,
            _ => Self::Days,
        }
    }

    /// The unit's symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Wh => "Wh",
            Self::J => "J",
            Self::M3 => "m³",
            Self::Kg => "kg",
            Self::W => "W",
            Self::JPerH => "J/h",
            Self::M3PerH => "m³/h",
            Self::M3PerMin => "m³/min",
            Self::M3PerS => "m³/s",
            Self::KgPerH => "kg/h",
            Self::Celsius => "°C",
            Self::Kelvin => "K",
            Self::Bar => "bar",
            Self::Seconds => "s",
            Self::Minutes => "min",
            Self::Hours => "h",
            Self::Days => "d",
        }
    }
}

impl fmt::Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// An exact measured quantity: `mantissa × 10^exponent` in `unit`. Never a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quantity {
    /// The raw value from the meter.
    pub mantissa: i128,
    /// The decimal exponent from the VIF.
    pub exponent: i8,
    /// The unit.
    pub unit: Option<Unit>,
}

impl Quantity {
    /// The value in units of `10^exponent`. `None` if it would need rounding or does
    /// not fit an `i128`.
    pub fn to_scale(self, exponent: i8) -> Option<i128> {
        let diff = i32::from(self.exponent) - i32::from(exponent);
        let p = 10i128.checked_pow(diff.unsigned_abs())?;
        if diff >= 0 {
            self.mantissa.checked_mul(p)
        } else {
            // `p` is a power of ten and never zero; the checked forms keep the panic path out.
            (self.mantissa.checked_rem(p)? == 0).then(|| self.mantissa.checked_div(p)).flatten()
        }
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut digits = [0u8; 40];
        let mut n = self.mantissa.unsigned_abs();
        let mut len = 0usize;
        loop {
            if let Some(slot) = digits.get_mut(len) {
                *slot = b'0' + u8::try_from(n % 10).unwrap_or(0);
            }
            len += 1;
            n /= 10;
            if n == 0 || len >= digits.len() {
                break;
            }
        }
        let digits = digits.get(..len).unwrap_or(&[]);
        if self.mantissa < 0 {
            f.write_str("-")?;
        }
        let exp = i32::from(self.exponent);
        let put = |f: &mut fmt::Formatter<'_>, d: &u8| write!(f, "{}", char::from(*d));
        if exp >= 0 {
            for d in digits.iter().rev() {
                put(f, d)?;
            }
            for _ in 0..exp {
                f.write_str("0")?;
            }
        } else {
            let frac = exp.unsigned_abs() as usize;
            if len <= frac {
                f.write_str("0.")?;
                for _ in len..frac {
                    f.write_str("0")?;
                }
                for d in digits.iter().rev() {
                    put(f, d)?;
                }
            } else {
                for (i, d) in digits.iter().rev().enumerate() {
                    if i == len - frac {
                        f.write_str(".")?;
                    }
                    put(f, d)?;
                }
            }
        }
        if let Some(unit) = self.unit {
            write!(f, " {unit}")?;
        }
        Ok(())
    }
}

/// A date, data type G (EN 13757-3 Annex A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    /// Year (2000-based; 0–127 → 2000–2127).
    pub year: u16,
    /// Month, 1–12.
    pub month: u8,
    /// Day, 1–31.
    pub day: u8,
}

impl Date {
    pub(crate) fn from_type_g(b: &[u8]) -> Self {
        let b0 = b.first().copied().unwrap_or(0);
        let b1 = b.get(1).copied().unwrap_or(0);
        let year = u16::from(((b0 & 0xE0) >> 5) | ((b1 & 0xF0) >> 1));
        Self { year: 2000 + year, month: b1 & 0x0F, day: b0 & 0x1F }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A date and time, data type F (EN 13757-3 Annex A). Local time of the meter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    /// The date.
    pub date: Date,
    /// Hour, 0–23.
    pub hour: u8,
    /// Minute, 0–59.
    pub minute: u8,
    /// The meter flags the time as invalid.
    pub invalid: bool,
    /// The meter flags the time as summer time.
    pub summer_time: bool,
}

impl DateTime {
    pub(crate) fn from_type_f(b: &[u8]) -> Self {
        let g = |i: usize| b.get(i).copied().unwrap_or(0);
        let (b0, b1, b2, b3) = (g(0), g(1), g(2), g(3));
        Self {
            date: Date::from_type_g(&[b2, b3]),
            hour: b1 & 0x1F,
            minute: b0 & 0x3F,
            invalid: b0 & 0x80 != 0,
            summer_time: b1 & 0x80 != 0,
        }
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {:02}:{:02}", self.date, self.hour, self.minute)?;
        if self.invalid {
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
    fn bcd_decoding() {
        assert_eq!(bcd(&[0x37, 0x18, 0x02]), Value::Bcd(21837));
        assert_eq!(bcd(&[0x13, 0x01]), Value::Bcd(113));
        assert_eq!(bcd(&[0x45, 0xF1]), Value::Bcd(-145));
        assert_eq!(bcd(&[0x4A, 0x01]), Value::InvalidBcd(&[0x4A, 0x01]));
        assert_eq!(bcd_u32([0x78, 0x56, 0x34, 0x12]), Some(12_345_678));
    }

    #[test]
    fn little_endian_integers() {
        assert_eq!(le_signed(&[0x15, 0x31, 0x00]), 12565);
        assert_eq!(le_signed(&[0xFF, 0xFF]), -1);
        assert_eq!(le_signed(&[0x00, 0x80]), -32768);
    }

    #[test]
    fn exact_display() {
        let q = |m, e| Quantity { mantissa: m, exponent: e, unit: Some(Unit::M3) }.to_string();
        assert_eq!(q(12565, -3), "12.565 m³");
        assert_eq!(q(21837, 1), "218370 m³");
        assert_eq!(q(5, -3), "0.005 m³");
    }

    #[test]
    fn dates() {
        // 2026-11-27: year 26 = 0b0011010 → b0 high bits 010, b1 high nibble 0011.
        let d = Date::from_type_g(&[0b0101_1011, 0b0011_1011]);
        assert_eq!(d.to_string(), "2026-11-27");
        let t = DateTime::from_type_f(&[30, 14, 0b0101_1011, 0b0011_1011]);
        assert_eq!(t.to_string(), "2026-11-27 14:30");
    }
}
