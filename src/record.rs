//! Data records (EN 13757-3): `DIB` (DIF + DIFEs), `VIB` (VIF + VIFEs), data.
//!
//! ```text
//! DIF  [DIFE…]  VIF  [VIFE…]  data
//! ```
//!
//! **DIF**: bit 7 extension, bit 6 storage number LSB, bits 5–4 function (instantaneous,
//! maximum, minimum, during error), bits 3–0 data field (length and coding).
//! **DIFE**: bit 7 extension, bit 6 subunit, bits 5–4 tariff, bits 3–0 storage number.
//! **VIF**: bit 7 extension, bits 6–0 what the value is and its decimal exponent
//! (see [`vif`](crate::vif)).
//!
//! Special DIFs: `2F` idle filler (skipped), `0F` manufacturer-specific data to the end,
//! `1F` the same and more records follow in the next telegram.

use core::fmt;

use crate::time::{Date, DateTime, Time};
use crate::value::{self, Quantity, Value};
use crate::vif::{Description, Extension, Kind, MAX_EXTENSIONS, Modifier, RecordError, Vib};
use crate::{Error, Result};

/// The function field of a DIF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    /// Instantaneous value.
    Instantaneous,
    /// Maximum value.
    Maximum,
    /// Minimum value.
    Minimum,
    /// Value during an error state.
    DuringError,
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Instantaneous => "instantaneous",
            Self::Maximum => "maximum",
            Self::Minimum => "minimum",
            Self::DuringError => "during error",
        })
    }
}

/// The data field of a DIF: how the value is coded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataField {
    /// No data.
    None,
    /// Two's-complement integer, little-endian, of this many bytes.
    Int(u8),
    /// 32-bit IEEE 754 real.
    Real32,
    /// BCD, little-endian, of this many bytes (2 digits each).
    Bcd(u8),
    /// Variable length, announced by an LVAR byte.
    Variable,
    /// Selection for readout (no data).
    Selection,
}

impl DataField {
    /// The number of data bytes, `None` for variable length.
    pub const fn size(self) -> Option<usize> {
        match self {
            Self::None | Self::Selection => Some(0),
            Self::Int(n) | Self::Bcd(n) => Some(n as usize),
            Self::Real32 => Some(4),
            Self::Variable => None,
        }
    }

    pub(crate) const fn from_nibble(n: u8) -> Self {
        match n & 0x0F {
            0x1 => Self::Int(1),
            0x2 => Self::Int(2),
            0x3 => Self::Int(3),
            0x4 => Self::Int(4),
            0x5 => Self::Real32,
            0x6 => Self::Int(6),
            0x7 => Self::Int(8),
            0x8 => Self::Selection,
            0x9 => Self::Bcd(1),
            0xA => Self::Bcd(2),
            0xB => Self::Bcd(3),
            0xC => Self::Bcd(4),
            0xD => Self::Variable,
            0xE => Self::Bcd(6),
            // 0x0, and 0xF which the caller handles as a special function.
            _ => Self::None,
        }
    }
}

/// The number of data bytes an LVAR byte announces (EN 13757-3 Table 6).
pub(crate) fn lvar_len(lvar: u8) -> Result<usize> {
    Ok(match lvar {
        0x00..=0xBF => usize::from(lvar),
        0xC0..=0xC9 => usize::from(lvar - 0xC0),
        0xD0..=0xD9 => usize::from(lvar - 0xD0),
        0xE0..=0xEF => usize::from(lvar - 0xE0),
        0xF0..=0xF4 => 4 * usize::from(lvar - 0xEC),
        0xF5 => 48,
        0xF6 => 64,
        other => return Err(Error::UnsupportedLvar(other)),
    })
}

/// The data information block: what kind of value, which storage, tariff and subunit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dib {
    /// The DIF byte.
    pub dif: u8,
    /// Instantaneous, maximum, minimum or during error.
    pub function: Function,
    /// How the value is coded.
    pub data: DataField,
    /// Storage number: 0 is the current value, higher numbers are historic values.
    pub storage: u64,
    /// Tariff.
    pub tariff: u32,
    /// Subunit (device unit).
    pub subunit: u16,
}

/// One data record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record<'a> {
    /// Data information.
    pub dib: Dib,
    /// Value information.
    pub vib: Vib<'a>,
    /// The value.
    pub value: Value<'a>,
    /// Whether every byte of this record came out of decryption. `false` for plaintext
    /// telegrams and for the unencrypted tail after a partially encrypted mode-5/7
    /// payload, which nothing protects. Decryption is confidentiality, not integrity:
    /// see [`Authenticity::Encrypted`](crate::app::Authenticity::Encrypted).
    pub from_ciphertext: bool,
    /// The DIF … VIFE bytes as transmitted (`None` with a plain-text unit).
    pub header: Option<&'a [u8]>,
}

impl Record<'_> {
    /// What the value is: kind, unit and exponent (see [`Vib::description`]).
    pub fn description(&self) -> Description {
        self.vib.description()
    }

    /// The compact profile this record carries (VIFE `1F`, `13` or `1E` and an LVAR
    /// payload), if it is one. Its base value is the record with the same storage,
    /// tariff, subunit and VIF without the profile VIFE.
    pub fn profile(&self) -> Option<crate::profile::Profile<'_>> {
        let direction = self.vib.modifiers().find_map(|m| match m {
            Modifier::CompactProfile(d) => Some(d),
            _ => None,
        })?;
        match self.value {
            Value::Text(b) | Value::Bytes(b) => crate::profile::Profile::parse(direction, b),
            _ => None,
        }
    }

    /// A short English name for what the record measures, e.g. `"volume flow"`.
    pub fn name(&self) -> &'static str {
        self.vib.description().kind.name()
    }

    /// The value as an exact [`Quantity`], with every multiplicative correction VIFE
    /// applied.
    ///
    /// `None` when the value is not an integer or valid BCD, when the description has
    /// no unit (identifiers, counters, dates), when a combinable VIFE changes the unit
    /// or meaning in a way this crate does not model ("per hour", "date of", "number
    /// of exceeds", additive offset, error, manufacturer-specific, reserved; see
    /// [`Modifier::keeps_quantity`]), or when the corrected exponent overflows. Never a
    /// value off by a correction factor (001-describe-every-record/SR-001).
    pub fn quantity(&self) -> Option<Quantity> {
        if let Value::Duration(q) = self.value {
            return Some(q);
        }
        let (mantissa, base) = self.value.as_decimal()?;
        let d = self.vib.description();
        // A plain-text unit is a quantity without a `Unit`: the text is the unit.
        let unit = match d.kind {
            Kind::PlainText => None,
            _ => Some(d.unit?),
        };
        let mut exponent = d.exponent.checked_add(base)?;
        for m in self.vib.modifiers() {
            match m {
                Modifier::Scale(e) => exponent = exponent.checked_add(e)?,
                m if m.keeps_quantity() => {}
                _ => return None,
            }
        }
        Some(Quantity { mantissa, exponent, unit })
    }
}

impl fmt::Display for Record<'_> {
    /// One line: `name · modifiers · function · storage · tariff · subunit — value`.
    /// Defaults (instantaneous, storage 0, tariff 0, subunit 0) are left out.
    ///
    /// ```
    /// # use mbus_kit::Records;
    /// let r = Records::new(&[0xDA, 0x02, 0x3B, 0x13, 0x01]).next().unwrap().unwrap();
    /// assert_eq!(r.to_string(), "volume flow · maximum · storage 5 — 0.113 m³/h");
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.vib.description();
        match (d.kind, self.vib.plain_text()) {
            // "value — 54.10 %RH"; with an unmodelled modifier "value in igal · per hour".
            (Kind::PlainText, Some(unit)) if self.quantity().is_none() => {
                f.write_str("value in ")?;
                value::escaped(f, unit.iter().rev())?;
            }
            (Kind::PlainText, Some(_)) => f.write_str("value")?,
            (kind, _) => write!(f, "{kind}")?,
        }
        // Applied corrections are in the value; "no error" says nothing.
        let shown = |m: &Modifier| !matches!(m, Modifier::Scale(_) | Modifier::Error(RecordError(0)));
        for m in self.vib.modifiers().filter(shown) {
            write!(f, " · {m}")?;
        }
        let dib = &self.dib;
        if dib.function != Function::Instantaneous {
            write!(f, " · {}", dib.function)?;
        }
        if dib.storage != 0 {
            write!(f, " · storage {}", dib.storage)?;
        }
        if dib.tariff != 0 {
            write!(f, " · tariff {}", dib.tariff)?;
        }
        if dib.subunit != 0 {
            write!(f, " · subunit {}", dib.subunit)?;
        }
        f.write_str(" — ")?;
        match (self.quantity(), self.value) {
            // A real is exact but long: show 9 significant digits, marked as rounded.
            (Some(q), Value::Real32(_)) => match q.significant(9) {
                Some(r) if r != q => write!(f, "≈ {r}")?,
                _ => write!(f, "{q}")?,
            },
            // Type M, relative: a time point given as an offset back from reception (OMS
            // Annex N N.13: "RX timestamp − 3 s").
            (Some(q), Value::Duration(_)) => write!(f, "{q} before reception")?,
            (Some(q), _) => write!(f, "{q}")?,
            (None, _) if self.profile().is_some() => {
                if let Some(p) = self.profile() {
                    write!(f, "{p}")?;
                }
            }
            (None, value) => write!(f, "{value}")?,
        }
        if let (Some(_), Some(text)) = (self.quantity().filter(|q| q.unit.is_none()), self.vib.plain_text()) {
            f.write_str(" ")?;
            value::escaped(f, text.iter().rev())?;
        }
        Ok(())
    }
}

/// The records of a payload, parsed one at a time without allocating.
///
/// Stops at the first error, and at a `0F`/`1F` DIF; [`Records::manufacturer_data`]
/// then returns the manufacturer-specific bytes that follow.
#[derive(Clone, Debug)]
pub struct Records<'a> {
    buf: &'a [u8],
    pos: usize,
    done: bool,
    manufacturer: Option<&'a [u8]>,
    more_follows: bool,
    ciphertext_end: usize,
}

impl<'a> Records<'a> {
    /// Records in `buf` (the bytes after the data header).
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0, done: false, manufacturer: None, more_follows: false, ciphertext_end: 0 }
    }

    /// Records whose first `ciphertext_end` bytes were decrypted.
    pub(crate) const fn decrypted(buf: &'a [u8], ciphertext_end: usize) -> Self {
        Self { buf, pos: 0, done: false, manufacturer: None, more_follows: false, ciphertext_end }
    }

    /// Manufacturer-specific bytes after a `0F`/`1F` DIF, once iteration has reached them.
    pub const fn manufacturer_data(&self) -> Option<&'a [u8]> {
        self.manufacturer
    }

    /// Whether a `1F` DIF announced more records in the next telegram.
    pub const fn more_follows(&self) -> bool {
        self.more_follows
    }

    fn byte(&mut self) -> Result<u8> {
        let b = self.buf.get(self.pos).copied().ok_or(Error::UnexpectedEnd)?;
        self.pos += 1;
        Ok(b)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::UnexpectedEnd)?;
        let s = self.buf.get(self.pos..end).ok_or(Error::UnexpectedEnd)?;
        self.pos = end;
        Ok(s)
    }

    fn record(&mut self, dif: u8) -> Result<Record<'a>> {
        let header_start = self.pos.saturating_sub(1);
        let function = match (dif >> 4) & 0x03 {
            0 => Function::Instantaneous,
            1 => Function::Maximum,
            2 => Function::Minimum,
            _ => Function::DuringError,
        };
        let mut storage = u64::from((dif >> 6) & 1);
        let mut tariff = 0u32;
        let mut subunit = 0u16;
        let mut ext = dif & 0x80 != 0;
        let mut i = 0u32;
        while ext {
            if i as usize >= MAX_EXTENSIONS {
                return Err(Error::TooManyExtensions);
            }
            let dife = self.byte()?;
            storage |= u64::from(dife & 0x0F) << (1 + 4 * i);
            tariff |= u32::from((dife >> 4) & 0x03) << (2 * i);
            subunit |= u16::from((dife >> 6) & 1) << i;
            ext = dife & 0x80 != 0;
            i += 1;
        }
        let data = DataField::from_nibble(dif);
        let dib = Dib { dif, function, data, storage, tariff, subunit };

        // A plain-text VIF `FC` puts its text after the VIFE chain (EN 13757-3:2018
        // Table C.2); some meters (Elvaco CMa10) put it straight after the VIF. Try the
        // standard first, the deviation if the standard does not parse.
        let start = self.pos;
        match self.value_part(dib, false, header_start) {
            Err(e) if self.buf.get(start) == Some(&0xFC) => {
                self.pos = start;
                self.value_part(dib, true, header_start).map_err(|_| e)
            }
            r => r,
        }
    }

    fn plain_text(&mut self) -> Result<&'a [u8]> {
        let n = self.byte()?;
        self.take(usize::from(n))
    }

    fn value_part(&mut self, dib: Dib, text_first: bool, header_start: usize) -> Result<Record<'a>> {
        let data = dib.data;
        let vif = self.byte()?;
        let mut plain_text = if text_first { Some(self.plain_text()?) } else { None };
        let mut vifes = [0u8; MAX_EXTENSIONS];
        let mut vife_len = 0u8;
        let mut ext = vif & 0x80 != 0;
        while ext {
            let vife = self.byte()?;
            *vifes.get_mut(usize::from(vife_len)).ok_or(Error::TooManyExtensions)? = vife;
            vife_len += 1;
            ext = vife & 0x80 != 0;
        }
        // DIF … VIFE: the layout a compact frame refers to (none with a plain-text unit).
        let header = (vif & 0x7F != 0x7C).then(|| self.buf.get(header_start..self.pos)).flatten();
        if vif & 0x7F == 0x7C && plain_text.is_none() {
            plain_text = Some(self.plain_text()?);
        }
        let vib = Vib::new(vif, vifes, vife_len, plain_text);
        let time_point =
            vib.description().kind.is_time_point() || vib.modifiers().any(Modifier::is_time_point);
        let unsigned = vib.modifiers().any(|m| m == Modifier::Extension(Extension::Unsigned));

        let value = match data {
            DataField::None | DataField::Selection => Value::None,
            DataField::Int(n) => {
                let bytes = self.take(usize::from(n))?;
                match (time_point, bytes) {
                    (true, &[b0, b1]) => Value::Date(Date::from_type_g(b0, b1)),
                    (true, &[b0, b1, b2]) => Value::Time(Time::from_type_j(b0, b1, b2)),
                    (true, &[b0, b1, b2, b3]) => Value::DateTime(DateTime::from_type_f([b0, b1, b2, b3])),
                    (true, &[b0, b1, b2, b3, b4, b5]) => {
                        Value::DateTime(DateTime::from_type_i([b0, b1, b2, b3, b4, b5]))
                    }
                    _ if unsigned => Value::UInt(value::le_unsigned(bytes)),
                    _ => Value::Int(value::le_signed(bytes)),
                }
            }
            DataField::Real32 => {
                let b = self.take(4)?;
                Value::Real32(u32::from_le_bytes(b.try_into().map_err(|_| Error::UnexpectedEnd)?))
            }
            DataField::Bcd(n) => value::bcd(self.take(usize::from(n))?),
            DataField::Variable => {
                let lvar = self.byte()?;
                match lvar {
                    0x00..=0xBF => Value::Text(self.take(usize::from(lvar))?),
                    0xC0..=0xC9 => value::bcd(self.take(usize::from(lvar - 0xC0))?),
                    0xD0..=0xD9 => value::bcd(self.take(usize::from(lvar - 0xD0))?).negate(),
                    0xE0..=0xEF => {
                        let bytes = self.take(usize::from(lvar - 0xE0))?;
                        if let Some(d) = time_point.then(|| value::type_m(bytes)).flatten() {
                            Value::Duration(d)
                        } else if bytes.len() <= 8 {
                            Value::Int(value::le_signed(bytes))
                        } else {
                            Value::Bytes(bytes)
                        }
                    }
                    // Binary numbers of 16, 20, 24, 28, 32 (4 × (LVAR − EC)), 48 and 64 bytes
                    // (EN 13757-3:2018 as read by libmbus and wmbusmeters).
                    0xF0..=0xF4 => Value::Bytes(self.take(4 * usize::from(lvar - 0xEC))?),
                    0xF5 => Value::Bytes(self.take(48)?),
                    0xF6 => Value::Bytes(self.take(64)?),
                    other => return Err(Error::UnsupportedLvar(other)),
                }
            }
        };
        Ok(Record { dib, vib, value, from_ciphertext: false, header })
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.done {
            let Some(&dif) = self.buf.get(self.pos) else {
                self.done = true;
                return None;
            };
            self.pos += 1;
            match dif {
                // 2F: idle filler; 7F: global readout request, a marker without data.
                0x2F | 0x7F => {}
                0x0F | 0x1F => {
                    self.more_follows = dif == 0x1F;
                    self.manufacturer = Some(self.buf.get(self.pos..).unwrap_or(&[]));
                    self.done = true;
                    return None;
                }
                d if d & 0x0F == 0x0F => {
                    self.done = true;
                    return Some(Err(Error::ReservedDif(d)));
                }
                d => {
                    let r = self.record(d).map(|mut r| {
                        r.from_ciphertext = self.pos <= self.ciphertext_end;
                        r
                    });
                    self.done = r.is_err();
                    return Some(r);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn storage_tariff_and_subunit_from_extensions() {
        // DA 02: storage LSB 1 + DIFE storage 2 → 5; maximum; 4-digit BCD.
        let mut r = Records::new(&[0xDA, 0x02, 0x3B, 0x13, 0x01]);
        let rec = r.next().unwrap().unwrap();
        assert_eq!(rec.dib.storage, 5);
        assert_eq!(rec.dib.function, Function::Maximum);
        assert_eq!(rec.dib.data, DataField::Bcd(2));
        assert_eq!(rec.value, Value::Bcd(113));
        // 8B 60: DIFE with subunit 1, tariff 2.
        let rec = Records::new(&[0x8B, 0x60, 0x04, 0x37, 0x18, 0x02]).next().unwrap().unwrap();
        assert_eq!((rec.dib.storage, rec.dib.tariff, rec.dib.subunit), (0, 2, 1));
    }

    #[test]
    fn filler_and_manufacturer_data() {
        let mut r = Records::new(&[0x2F, 0x2F, 0x01, 0xFD, 0x08, 0x05, 0x0F, 0xAA, 0xBB]);
        let rec = r.next().unwrap().unwrap();
        assert_eq!(rec.vib.vif(), 0xFD);
        assert_eq!(rec.vib.vifes(), &[0x08]);
        assert_eq!(rec.name(), "access number");
        assert_eq!(rec.value, Value::Int(5));
        assert!(r.next().is_none());
        assert_eq!(r.manufacturer_data(), Some(&[0xAA, 0xBB][..]));
        assert!(!r.more_follows());
    }

    #[test]
    fn plain_text_vif() {
        // 01 FC 03? no: VIF 7C, then length 3, then "hPa" reversed, then 1-byte int.
        let rec = Records::new(&[0x01, 0x7C, 0x03, b'a', b'P', b'h', 0x2A]).next().unwrap().unwrap();
        assert_eq!(rec.vib.plain_text(), Some(&b"aPh"[..]));
        assert_eq!(rec.to_string(), "value — 42 hPa");
        assert_eq!(rec.value, Value::Int(42));
    }

    #[test]
    fn plain_text_after_vifes_and_the_elvaco_deviation() {
        // EN 13757-3:2018 Table C.2: 0C FC A2 73 04 "lagi" 26 08 42 75 = 75420.826 igal/h.
        let rec =
            Records::new(&[0x0C, 0xFC, 0xA2, 0x73, 0x04, b'l', b'a', b'g', b'i', 0x26, 0x08, 0x42, 0x75])
                .next()
                .unwrap()
                .unwrap();
        assert_eq!(rec.vib.plain_text(), Some(&b"lagi"[..]));
        assert_eq!(rec.value, Value::Bcd(75_420_826));
        assert_eq!(rec.to_string(), "value in igal · per hour — 75420826");
        // Elvaco CMa10 (libmbus test frame): 02 FC 03 "HR%" 74 22 15 = 54.10 %RH.
        let rec =
            Records::new(&[0x02, 0xFC, 0x03, b'H', b'R', b'%', 0x74, 0x22, 0x15]).next().unwrap().unwrap();
        assert_eq!(rec.vib.plain_text(), Some(&b"HR%"[..]));
        assert_eq!(rec.vib.vifes(), &[0x74]);
        assert_eq!(rec.value, Value::Int(0x1522));
    }

    #[test]
    fn too_many_extensions() {
        let input = [0x84u8, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00];
        assert_eq!(Records::new(&input).next(), Some(Err(Error::TooManyExtensions)));
    }

    #[test]
    fn truncation_is_an_error() {
        let full = [0x04u8, 0x13, 0x01, 0x02, 0x03, 0x04];
        for cut in 1..full.len() {
            assert!(matches!(Records::new(&full[..cut]).next(), Some(Err(_))), "cut {cut}");
        }
    }
}
