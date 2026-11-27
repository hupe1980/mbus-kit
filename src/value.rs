use core::fmt;

use crate::time::{Date, DateTime, Time};

/// A record's value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value<'a> {
    /// No data.
    None,
    /// A binary integer (data type B, two's complement).
    Int(i64),
    /// An unsigned binary integer (data type C, announced by the combinable extension
    /// VIFE `FC 11`).
    UInt(u64),
    /// A BCD number.
    Bcd(i64),
    /// A BCD field with digits outside 0–9 (meters use this to signal errors). The raw
    /// bytes, as transmitted.
    InvalidBcd(&'a [u8]),
    /// A 32-bit IEEE 754 real, as raw bits. [`Value::as_decimal`] and
    /// [`Record::quantity`](crate::Record::quantity) give its exact decimal value; no
    /// float is involved.
    Real32(u32),
    /// A date (type G).
    Date(Date),
    /// A time of day (type J).
    Time(Time),
    /// A date and time (type F or I).
    DateTime(DateTime),
    /// A text (variable length, LVAR `00`–`BF`), as transmitted (M-Bus sends it reversed).
    Text(&'a [u8]),
    /// Binary data longer than 8 bytes (LVAR `E9`–`EF`, `F0`–`F6`).
    Bytes(&'a [u8]),
    /// A duration in seconds, exact: data type M in its relative form (EN 13757-3:2018
    /// Table A.12, e.g. "offset 3 s" in OMS Annex N N.13).
    Duration(Quantity),
}

impl fmt::Display for Value<'_> {
    /// Integers in decimal, dates ISO-8601-like, text in reading order. Bytes from the
    /// meter are never written raw: anything but printable ASCII is escaped as `\xNN`,
    /// so a crafted telegram cannot put control sequences into a terminal or log
    /// (001-describe-every-record/SR-003).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::None => f.write_str("—"),
            Self::Int(v) | Self::Bcd(v) => write!(f, "{v}"),
            Self::UInt(v) => write!(f, "{v}"),
            Self::InvalidBcd(b) => {
                f.write_str("invalid BCD ")?;
                hex(f, b.iter().rev())
            }
            Self::Real32(bits) => match real32_exact(bits) {
                Some((mantissa, exponent)) => write!(f, "{}", Quantity { mantissa, exponent, unit: None }),
                None => write!(f, "IEEE 754 0x{bits:08X}"),
            },
            Self::Date(d) => write!(f, "{d}"),
            Self::Time(t) => write!(f, "{t}"),
            Self::DateTime(t) => write!(f, "{t}"),
            Self::Text(t) => escaped(f, t.iter().rev()),
            Self::Bytes(b) => hex(f, b.iter()),
            Self::Duration(q) => write!(f, "{q}"),
        }
    }
}

/// Printable ASCII as is, everything else as `\xNN`.
pub(crate) fn escaped<'b>(f: &mut fmt::Formatter<'_>, bytes: impl Iterator<Item = &'b u8>) -> fmt::Result {
    for &b in bytes {
        if b == b' ' || b.is_ascii_graphic() && b != b'\\' {
            write!(f, "{}", char::from(b))?;
        } else {
            write!(f, "\\x{b:02x}")?;
        }
    }
    Ok(())
}

fn hex<'b>(f: &mut fmt::Formatter<'_>, bytes: impl Iterator<Item = &'b u8>) -> fmt::Result {
    for &b in bytes {
        write!(f, "{b:02X}")?;
    }
    Ok(())
}

impl Value<'_> {
    /// The value as an exact decimal `mantissa × 10^exponent`: integers and valid BCD
    /// with exponent 0, and 32-bit reals converted exactly (every finite IEEE 754
    /// single is a finite decimal). `None` for NaN, infinities, reals too large
    /// or too small for an `i128` mantissa, and non-numeric values.
    pub fn as_decimal(&self) -> Option<(i128, i8)> {
        match *self {
            Self::Real32(bits) => real32_exact(bits),
            _ => self.as_integer().map(|m| (m, 0)),
        }
    }

    /// The value as an integer, for binary integers and valid BCD.
    pub const fn as_integer(&self) -> Option<i128> {
        match *self {
            Self::Int(v) | Self::Bcd(v) => Some(v as i128),
            Self::UInt(v) => Some(v as i128),
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

/// The exact decimal value of an IEEE 754 single: `m × 2^e` is `m × 5^-e × 10^e`.
fn real32_exact(bits: u32) -> Option<(i128, i8)> {
    let negative = bits >> 31 != 0;
    let biased = (bits >> 23) & 0xFF;
    let fraction = bits & 0x007F_FFFF;
    if biased == 0xFF {
        return None; // infinity or NaN
    }
    let (mut m, mut e) = if biased == 0 {
        (i128::from(fraction), -149i32)
    } else {
        (i128::from(fraction | 0x0080_0000), i32::try_from(biased).ok()? - 150)
    };
    if m == 0 {
        return Some((0, 0));
    }
    while m % 2 == 0 {
        m /= 2;
        e += 1;
    }
    let (mantissa, exponent) = if e >= 0 {
        (m.checked_mul(2i128.checked_pow(e.unsigned_abs())?)?, 0)
    } else {
        let k = e.unsigned_abs();
        (m.checked_mul(5i128.checked_pow(k)?)?, i8::try_from(e).ok()?)
    };
    Some((if negative { -mantissa } else { mantissa }, exponent))
}

/// Little-endian two's complement of 1 to 8 bytes.
pub(crate) fn le_signed(bytes: &[u8]) -> i64 {
    let negative = bytes.last().is_some_and(|b| b & 0x80 != 0);
    let start: u64 = if negative { u64::MAX } else { 0 };
    let raw = bytes.iter().rev().fold(start, |acc, &b| (acc << 8) | u64::from(b));
    i64::from_le_bytes(raw.to_le_bytes())
}

/// Data type M (LVAR, 2–10 bytes): a two's-complement count, then a header byte with
/// the resolution (bits 6–5: 2 s, 1 s, 1/256 s, 1/32768 s) and the time offset (bits 4–0).
/// Only the relative form (offset `10000b`) is decoded; the absolute form's epoch is not
/// public, so it stays raw. The most negative count means invalid.
pub(crate) fn type_m(bytes: &[u8]) -> Option<Quantity> {
    let (&header, count) = bytes.split_last()?;
    if header & 0x1F != 0x10 || count.is_empty() || count.len() > 8 {
        return None;
    }
    let c = i128::from(le_signed(count));
    let min = -(1i128 << (8 * count.len() - 1));
    if c == min {
        return None;
    }
    // 2 s, 1 s, 1/256 s = 390625 × 10^-8 s, 1/32768 s = 30517578125 × 10^-15 s: exact.
    let (factor, exponent) = match (header >> 5) & 0x03 {
        0 => (2, 0),
        1 => (1, 0),
        2 => (390_625, -8),
        _ => (30_517_578_125, -15),
    };
    // A duration has no register resolution to keep: drop trailing zeros (still exact).
    let (mut mantissa, mut exponent) = (c.checked_mul(factor)?, exponent);
    while exponent < 0 && mantissa % 10 == 0 {
        mantissa /= 10;
        exponent += 1;
    }
    Some(Quantity { mantissa, exponent, unit: Some(Unit::Seconds) })
}

/// Little-endian unsigned integer of 1 to 8 bytes.
pub(crate) fn le_unsigned(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
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
    /// Month (calendar).
    Months,
    /// Year (calendar).
    Years,
    /// Volt.
    Volt,
    /// Ampere.
    Ampere,
    /// Hertz.
    Hertz,
    /// Percent (relative humidity).
    Percent,
    /// Degree Fahrenheit.
    Fahrenheit,
    /// Calorie.
    Calorie,
    /// Cubic foot.
    CubicFeet,
    /// US gallon.
    UsGallon,
    /// US gallon per minute.
    UsGallonPerMin,
    /// US gallon per hour.
    UsGallonPerH,
    /// Volt-ampere (apparent power).
    VoltAmpere,
    /// Volt-ampere reactive (reactive power).
    Var,
    /// Volt-ampere hour (apparent energy).
    VaH,
    /// Volt-ampere reactive hour (reactive energy).
    VarH,
    /// Degree of angle (phase).
    Degree,
    /// Decibel-milliwatt (signal level).
    Dbm,
    /// Units of the local legal currency.
    Currency,
    /// Baud.
    Baud,
    /// Bit times (response delay).
    BitTimes,
    /// Parts per million.
    Ppm,
    /// Parts per billion.
    Ppb,
    /// Microgram per cubic metre.
    MicrogramPerM3,
    /// Count per cubic metre.
    PerM3,
    /// Lux.
    Lux,
    /// Candela.
    Candela,
    /// Watt per square metre.
    WattPerM2,
    /// Metre per second.
    MetrePerS,
    /// A-weighted decibel.
    Dba,
    /// Formazin nephelometric unit (turbidity).
    Fnu,
    /// pH.
    Ph,
    /// Millimetre.
    Millimetre,
}

impl Unit {
    /// Seconds, minutes, hours or days from the low two bits of a code.
    pub(crate) const fn duration(nn: u8) -> Self {
        match nn & 0x03 {
            0 => Self::Seconds,
            1 => Self::Minutes,
            2 => Self::Hours,
            _ => Self::Days,
        }
    }

    /// Hours, days, months or years from the low two bits (`FD 68`–`6F`).
    pub(crate) const fn long_duration(pp: u8) -> Self {
        match pp & 0x03 {
            0 => Self::Hours,
            1 => Self::Days,
            2 => Self::Months,
            _ => Self::Years,
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
            Self::Months => "months",
            Self::Years => "years",
            Self::Volt => "V",
            Self::Ampere => "A",
            Self::Hertz => "Hz",
            Self::Percent => "%",
            Self::Fahrenheit => "°F",
            Self::Calorie => "cal",
            Self::CubicFeet => "ft³",
            Self::UsGallon => "US gal",
            Self::UsGallonPerMin => "US gal/min",
            Self::UsGallonPerH => "US gal/h",
            Self::VoltAmpere => "VA",
            Self::Var => "var",
            Self::VaH => "VAh",
            Self::VarH => "varh",
            Self::Degree => "°",
            Self::Dbm => "dBm",
            Self::Currency => "currency units",
            Self::Baud => "Bd",
            Self::BitTimes => "bit times",
            Self::Ppm => "ppm",
            Self::Ppb => "ppb",
            Self::MicrogramPerM3 => "µg/m³",
            Self::PerM3 => "/m³",
            Self::Lux => "lx",
            Self::Candela => "cd",
            Self::WattPerM2 => "W/m²",
            Self::MetrePerS => "m/s",
            Self::Dba => "dB(A)",
            Self::Fnu => "FNU",
            Self::Ph => "pH",
            Self::Millimetre => "mm",
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

impl Quantity {
    /// Round to units of `10^exponent`, half to even: an explicit, visible rounding for
    /// display or for a coarser register. Exact (no rounding) when `exponent` is not
    /// coarser than the quantity's. `None` on overflow.
    pub fn round(self, exponent: i8) -> Option<Self> {
        if exponent <= self.exponent {
            return Some(Self { mantissa: self.to_scale(exponent)?, exponent, unit: self.unit });
        }
        let diff = i32::from(exponent) - i32::from(self.exponent);
        let p = 10i128.checked_pow(diff.unsigned_abs())?;
        let q = self.mantissa.checked_div(p)?;
        let r = self.mantissa.checked_rem(p)?;
        let twice = r.unsigned_abs().checked_mul(2)?;
        let half = p.unsigned_abs();
        let away = twice > half || (twice == half && q % 2 != 0);
        let mantissa = if away { q.checked_add(r.signum())? } else { q };
        Some(Self { mantissa, exponent, unit: self.unit })
    }

    /// Rounded to at most `digits` significant digits (never more precise than it is).
    pub fn significant(self, digits: u32) -> Option<Self> {
        let len = self.mantissa.unsigned_abs().checked_ilog10().map_or(1, |l| l + 1);
        if len <= digits {
            return Some(self);
        }
        let shift = i8::try_from(len - digits).ok()?;
        self.round(self.exponent.checked_add(shift)?)
    }
}

impl Unit {
    /// Units shown with an SI prefix (k, M, G, T) when large: energy and power.
    const fn prefixable(self) -> bool {
        matches!(
            self,
            Self::Wh
                | Self::J
                | Self::W
                | Self::JPerH
                | Self::VoltAmpere
                | Self::Var
                | Self::VaH
                | Self::VarH
                | Self::Calorie
        )
    }
}

impl fmt::Display for Quantity {
    /// Exact decimal notation: `12.565 m³`. Energy and power of 1000 or more get an SI
    /// prefix, still exact: `218.370 kWh`, `37.402 MWh`. The alternate form (`{:#}`)
    /// keeps the base unit: `218370 Wh`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(unit) = self.unit.filter(|u| u.prefixable() && !f.alternate() && self.mantissa != 0) {
            let len = self.mantissa.unsigned_abs().checked_ilog10().unwrap_or(0);
            let magnitude = i32::try_from(len).unwrap_or(0) + i32::from(self.exponent);
            let k = (magnitude / 3).clamp(0, 4);
            if k > 0 {
                let prefix =
                    ["", "k", "M", "G", "T"].get(usize::try_from(k).unwrap_or(0)).copied().unwrap_or("");
                let shift = i8::try_from(3 * k).unwrap_or(0);
                let scaled = Self {
                    mantissa: self.mantissa,
                    exponent: self.exponent.saturating_sub(shift),
                    unit: None,
                };
                return write!(f, "{scaled} {prefix}{unit}");
            }
        }
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
        if self.mantissa == 0 && exp >= 0 {
            f.write_str("0")?;
        } else if exp >= 0 {
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
    fn reals_are_exact_decimals() {
        // baylanheat (wmbusmeters): 0x46529447 = 13477.0693359375 exactly.
        assert_eq!(real32_exact(0x4652_9447), Some((134_770_693_359_375, -10)));
        assert_eq!(real32_exact(0x3F80_0000), Some((1, 0)));
        assert_eq!(real32_exact(0xC000_0000), Some((-2, 0)));
        assert_eq!(real32_exact(0x0000_0000), Some((0, 0)));
        assert_eq!(real32_exact(0x7F80_0000), None); // infinity
        assert_eq!(real32_exact(0x7FC0_0000), None); // NaN
        assert_eq!(real32_exact(0x0000_0001), None); // 2^-149: mantissa too long
        assert_eq!(real32_exact(0x7F7F_FFFF), None); // 3.4e38 does not fit i128
    }

    #[test]
    fn type_m_relative_durations() {
        // wmbusmeters test_type_m (EN 13757-3:2018 Table A.12 examples) and OMS N.13.
        let d = |b: &[u8]| type_m(b).map(|q| q.to_string());
        assert_eq!(d(&[0x03, 0x30]).as_deref(), Some("3 s"));
        assert_eq!(d(&[0x80, 0xDD, 0x50]).as_deref(), Some("-34.5 s"));
        assert_eq!(d(&[0x0A, 0x10]).as_deref(), Some("20 s"));
        assert_eq!(d(&[0x00, 0x40, 0x70]).as_deref(), Some("0.5 s"));
        assert_eq!(d(&[0x03, 0xB0]).as_deref(), Some("3 s"));
        assert_eq!(d(&[0x81, 0x30]).as_deref(), Some("-127 s"));
        assert_eq!(d(&[0x80, 0x30]), None, "most negative count: invalid");
        assert_eq!(d(&[0x0B, 0x60, 0x01, 0x00, 0x21]), None, "absolute form stays raw");
    }

    #[test]
    fn rounding_is_explicit_and_half_even() {
        let q = |m, e| Quantity { mantissa: m, exponent: e, unit: None };
        assert_eq!(q(218_370, 0).round(3), Some(q(218, 3)));
        assert_eq!(q(2_185, 0).round(1), Some(q(218, 1)));
        assert_eq!(q(2_175, 0).round(1), Some(q(218, 1)));
        assert_eq!(q(-2_185, 0).round(1), Some(q(-218, 1)));
        assert_eq!(q(12, 0).round(-2), Some(q(1200, -2)));
        assert_eq!(q(134_770_693_359_375, -10).significant(9).unwrap().to_string(), "13477.0693");
    }

    #[test]
    fn exact_display() {
        let q = |m, e| Quantity { mantissa: m, exponent: e, unit: Some(Unit::M3) }.to_string();
        assert_eq!(q(12565, -3), "12.565 m³");
        assert_eq!(q(21837, 1), "218370 m³");
        assert_eq!(q(5, -3), "0.005 m³");
        assert_eq!(q(0, 3), "0 m³");
        assert_eq!(q(0, -2), "0.00 m³");
        let e = |m, e| Quantity { mantissa: m, exponent: e, unit: Some(Unit::Wh) };
        assert_eq!(e(218_370, 0).to_string(), "218.370 kWh");
        assert_eq!(std::format!("{:#}", e(218_370, 0)), "218370 Wh");
        assert_eq!(e(37_402, 3).to_string(), "37.402 MWh");
        assert_eq!(e(999, 0).to_string(), "999 Wh");
        assert_eq!(e(-1_364, 0).to_string(), "-1.364 kWh");
        assert_eq!(e(0, 3).to_string(), "0 Wh");
    }
}
