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
//! (see [`Vib::describe`]).
//!
//! Special DIFs: `2F` idle filler (skipped), `0F` manufacturer-specific data to the end,
//! `1F` the same and more records follow in the next telegram.

use crate::value::{self, Quantity, Unit, Value};
use crate::{Error, Result};

const MAX_EXTENSIONS: usize = 10;

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
    const fn from_nibble(n: u8) -> Self {
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

/// The value information block: what the value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vib<'a> {
    /// The VIF byte.
    pub vif: u8,
    vifes: [u8; MAX_EXTENSIONS],
    vife_len: u8,
    /// For a plain-text VIF (`7C`/`FC`): the unit text, as transmitted (reversed).
    pub plain_text: Option<&'a [u8]>,
}

/// What a VIF says about a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Description {
    /// A physical quantity in `unit`, scaled by `10^exponent`.
    Quantity {
        /// What is measured.
        name: &'static str,
        /// The unit.
        unit: Unit,
        /// The decimal exponent.
        exponent: i8,
    },
    /// A duration in `unit` (seconds, minutes, hours or days).
    Duration {
        /// What is measured.
        name: &'static str,
        /// The unit.
        unit: Unit,
    },
    /// A date (type G).
    Date,
    /// A date and time (type F).
    DateTime,
    /// Something without a unit: fabrication number, identification, bus address,
    /// heat cost allocator units.
    Other(&'static str),
    /// The unit is given as text (VIF `7C`).
    PlainText,
    /// An extension table (`FB`, `FD`): the meaning is in the first VIFE, which this
    /// version does not interpret yet.
    Extension(u8),
    /// Manufacturer-specific (`7F`/`FF`) or any (`7E`/`FE`).
    Manufacturer,
}

impl Vib<'_> {
    /// The VIFE bytes.
    pub fn vifes(&self) -> &[u8] {
        self.vifes.get(..usize::from(self.vife_len)).unwrap_or(&[])
    }

    /// What the primary VIF says (EN 13757-3, Table 10). VIFEs that modify the meaning
    /// are not applied yet; check [`Vib::vifes`] when exactness matters.
    pub fn describe(&self) -> Description {
        let v = self.vif & 0x7F;
        let n = v & 0x07;
        let nn = v & 0x03;
        // `n` and `nn` are at most 7, so the subtractions stay in i8 range.
        let e = |offset: i8| i8::try_from(n).unwrap_or(0) - offset;
        let ee = |offset: i8| i8::try_from(nn).unwrap_or(0) - offset;
        let duration = |name| Description::Duration { name, unit: Unit::duration(nn) };
        let q = |name, unit, exponent| Description::Quantity { name, unit, exponent };
        match v {
            0x00..=0x07 => q("energy", Unit::Wh, e(3)),
            0x08..=0x0F => q("energy", Unit::J, e(0)),
            0x10..=0x17 => q("volume", Unit::M3, e(6)),
            0x18..=0x1F => q("mass", Unit::Kg, e(3)),
            0x20..=0x23 => duration("on time"),
            0x24..=0x27 => duration("operating time"),
            0x28..=0x2F => q("power", Unit::W, e(3)),
            0x30..=0x37 => q("power", Unit::JPerH, e(0)),
            0x38..=0x3F => q("volume flow", Unit::M3PerH, e(6)),
            0x40..=0x47 => q("volume flow", Unit::M3PerMin, e(7)),
            0x48..=0x4F => q("volume flow", Unit::M3PerS, e(9)),
            0x50..=0x57 => q("mass flow", Unit::KgPerH, e(3)),
            0x58..=0x5B => q("flow temperature", Unit::Celsius, ee(3)),
            0x5C..=0x5F => q("return temperature", Unit::Celsius, ee(3)),
            0x60..=0x63 => q("temperature difference", Unit::Kelvin, ee(3)),
            0x64..=0x67 => q("external temperature", Unit::Celsius, ee(3)),
            0x68..=0x6B => q("pressure", Unit::Bar, ee(3)),
            0x6C => Description::Date,
            0x6D => Description::DateTime,
            0x6E => Description::Other("units for heat cost allocator"),
            0x70..=0x73 => duration("averaging duration"),
            0x74..=0x77 => duration("actuality duration"),
            0x78 => Description::Other("fabrication number"),
            0x79 => Description::Other("identification"),
            0x7A => Description::Other("bus address"),
            0x7C => Description::PlainText,
            0x7B | 0x7D => Description::Extension(self.vif),
            _ => Description::Manufacturer,
        }
    }
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
}

impl Record<'_> {
    /// The value as an exact [`Quantity`] with unit and exponent, when the VIF describes
    /// a physical quantity or duration and the value is an integer or valid BCD.
    pub fn quantity(&self) -> Option<Quantity> {
        let mantissa = self.value.as_integer()?;
        match self.vib.describe() {
            Description::Quantity { unit, exponent, .. } => {
                Some(Quantity { mantissa, exponent, unit: Some(unit) })
            }
            Description::Duration { unit, .. } => Some(Quantity { mantissa, exponent: 0, unit: Some(unit) }),
            _ => None,
        }
    }

    /// A short name for what the record measures.
    pub fn name(&self) -> &'static str {
        match self.vib.describe() {
            Description::Quantity { name, .. }
            | Description::Duration { name, .. }
            | Description::Other(name) => name,
            Description::Date => "date",
            Description::DateTime => "date and time",
            Description::PlainText => "plain-text unit",
            Description::Extension(_) => "extension VIF",
            Description::Manufacturer => "manufacturer specific",
        }
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
}

impl<'a> Records<'a> {
    /// Records in `buf` (the bytes after the data header).
    pub const fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0, done: false, manufacturer: None, more_follows: false }
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

        let vif = self.byte()?;
        let mut vifes = [0u8; MAX_EXTENSIONS];
        let mut vife_len = 0u8;
        let mut ext = vif & 0x80 != 0;
        while ext {
            let vife = self.byte()?;
            *vifes.get_mut(usize::from(vife_len)).ok_or(Error::TooManyExtensions)? = vife;
            vife_len += 1;
            ext = vife & 0x80 != 0;
        }
        let plain_text = if vif & 0x7F == 0x7C {
            let n = self.byte()?;
            Some(self.take(usize::from(n))?)
        } else {
            None
        };
        let vib = Vib { vif, vifes, vife_len, plain_text };

        let value = match data {
            DataField::None | DataField::Selection => Value::None,
            DataField::Int(n) => {
                let bytes = self.take(usize::from(n))?;
                if vif & 0x7F == 0x6C && n == 2 {
                    Value::Date(value::Date::from_type_g(bytes))
                } else if vif & 0x7F == 0x6D && n == 4 {
                    Value::DateTime(value::DateTime::from_type_f(bytes))
                } else {
                    Value::Int(value::le_signed(bytes))
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
                        if bytes.len() <= 8 {
                            Value::Int(value::le_signed(bytes))
                        } else {
                            Value::Bytes(bytes)
                        }
                    }
                    other => return Err(Error::UnsupportedLvar(other)),
                }
            }
        };
        Ok(Record { dib, vib, value })
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
                    let r = self.record(d);
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
    use super::*;

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
        assert_eq!(rec.vib.vif, 0xFD);
        assert_eq!(rec.vib.vifes(), &[0x08]);
        assert_eq!(rec.value, Value::Int(5));
        assert!(r.next().is_none());
        assert_eq!(r.manufacturer_data(), Some(&[0xAA, 0xBB][..]));
        assert!(!r.more_follows());
    }

    #[test]
    fn plain_text_vif() {
        // 01 FC 03? no: VIF 7C, then length 3, then "hPa" reversed, then 1-byte int.
        let rec = Records::new(&[0x01, 0x7C, 0x03, b'a', b'P', b'h', 0x2A]).next().unwrap().unwrap();
        assert_eq!(rec.vib.plain_text, Some(&b"aPh"[..]));
        assert_eq!(rec.value, Value::Int(42));
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
