//! Compact profiles (EN 13757-3:2018 Annex F.2, OMS Vol. 2 Annex G): a series of values
//! in one record, e.g. a heat cost allocator's monthly history.
//!
//! ```text
//! DIF(…D)  DIFE…  VIF  VIFE…  1F|13|1E   LVAR  spacing-control  spacing-value  element…
//! ```
//!
//! The profile belongs to a **base value** (the record with the same storage, tariff,
//! subunit and VIF without the profile VIFE) and a **base time** (the date record of that
//! storage). Element *k* (0-based) lies *k* + 1 spacings after the base time (`1F`, `1E`)
//! or before it (`13`): [`Profile::offset`], [`Profile::date`]. [`Profile::values`] turns
//! the elements into readings: absolute elements are the readings; increments accumulate
//! from the base value.

use core::fmt;

use crate::record::DataField;
use crate::time::Date;
use crate::value::{self, Unit, Value};
use crate::vif::Direction;

/// How the elements relate to the readings (spacing control bits 7–6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Increment {
    /// Each element is a reading (signed).
    Absolute,
    /// Each element is a positive increment (unsigned).
    Positive,
    /// Each element is a negative increment (unsigned).
    Negative,
    /// Each element is the younger minus the older reading (signed).
    Difference,
}

/// The time between elements (spacing value with its unit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Spacing {
    /// `n` seconds, minutes, hours or days.
    Every(u8, Unit),
    /// Half a month.
    HalfMonth,
    /// 1, 3 or 6 months.
    Months(u8),
    /// Not spaced in time: the elements are array columns.
    Columns,
    /// A reserved spacing value.
    Reserved(u8),
}

/// How far an element lies from the profile's base time; negative is earlier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Offset {
    /// Seconds (spacing in seconds, minutes, hours or days).
    Seconds(i64),
    /// Whole months.
    Months(i32),
    /// Half months.
    HalfMonths(i32),
}

/// A compact profile record's payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile<'a> {
    /// Forward, inverse, or with register numbers.
    pub direction: Direction,
    /// What the elements mean.
    pub increment: Increment,
    /// The time between elements.
    pub spacing: Spacing,
    /// How each element is coded.
    pub element: DataField,
    elements: &'a [u8],
}

impl<'a> Profile<'a> {
    /// Parse the LVAR payload of a profile record. `None` for a malformed one.
    pub(crate) fn parse(direction: Direction, payload: &'a [u8]) -> Option<Self> {
        let (&[control, value], elements) = payload.split_first_chunk::<2>()?;
        let element = DataField::from_nibble(control & 0x0F);
        let size = element.size()?;
        if size == 0 || elements.len() % size != 0 {
            return None;
        }
        let unit = Unit::duration((control >> 4) & 0x03);
        let spacing = match (value, (control >> 4) & 0x03) {
            (0, _) => Spacing::Columns,
            (1..=250, _) => Spacing::Every(value, unit),
            (253, 3) => Spacing::HalfMonth,
            (254, 3) => Spacing::Months(1),
            (254, 2) => Spacing::Months(3),
            (254, 1) => Spacing::Months(6),
            (v, _) => Spacing::Reserved(v),
        };
        let increment = match control >> 6 {
            0 => Increment::Absolute,
            1 => Increment::Positive,
            2 => Increment::Negative,
            _ => Increment::Difference,
        };
        Some(Self { direction, increment, spacing, element, elements })
    }

    /// The number of elements.
    pub fn len(&self) -> usize {
        self.element.size().and_then(|s| self.elements.len().checked_div(s)).unwrap_or(0)
    }

    /// Whether the profile has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Element `k` (0-based) as transmitted, `None` if it is out of range or holds the
    /// "illegal" preset value (unused or overflowed).
    pub fn element(&self, k: usize) -> Option<i128> {
        let size = self.element.size()?;
        let bytes = self.elements.get(k * size..(k + 1) * size)?;
        let signed = matches!(self.increment, Increment::Absolute | Increment::Difference);
        match self.element {
            DataField::Int(_) if signed => {
                let v = i128::from(value::le_signed(bytes));
                (v != -(1i128 << (8 * size - 1))).then_some(v)
            }
            DataField::Int(_) => {
                let v = i128::from(value::le_unsigned(bytes));
                (v != (1i128 << (8 * size)) - 1).then_some(v)
            }
            DataField::Bcd(_) => match value::bcd(bytes) {
                Value::Bcd(v) => Some(i128::from(v)),
                _ => None,
            },
            _ => None,
        }
    }

    /// When element `k` (0-based) was taken, relative to the base time: `k + 1` spacings
    /// later, or earlier for an inverse profile. `None` for elements that are columns,
    /// not points in time, a reserved spacing, or `k` out of range.
    pub fn offset(&self, k: usize) -> Option<Offset> {
        if k >= self.len() {
            return None;
        }
        let steps = i64::try_from(k).ok()?.checked_add(1)?;
        let steps = if self.direction == Direction::Inverse { -steps } else { steps };
        Some(match self.spacing {
            Spacing::Every(n, unit) => {
                let seconds = match unit {
                    Unit::Minutes => 60,
                    Unit::Hours => 3600,
                    Unit::Days => 86_400,
                    _ => 1,
                };
                Offset::Seconds(steps.checked_mul(i64::from(n) * seconds)?)
            }
            Spacing::Months(n) => Offset::Months(i32::try_from(steps.checked_mul(i64::from(n))?).ok()?),
            Spacing::HalfMonth => Offset::HalfMonths(i32::try_from(steps).ok()?),
            Spacing::Columns | Spacing::Reserved(_) => return None,
        })
    }

    /// The date of element `k` for a month-spaced profile, from the base record's date
    /// (month ends kept, OMS Vol. 2 Table G.5). `None` for other spacings.
    pub fn date(&self, k: usize, base: Date) -> Option<Date> {
        match self.offset(k)? {
            Offset::Months(m) => base.add_months(m),
            _ => None,
        }
    }

    /// The readings, element by element, from the base value (in the base record's
    /// mantissa: apply its unit and exponent). `None` for an illegal element; the series
    /// continues from the last valid reading.
    pub fn values(&self, base: i128) -> impl Iterator<Item = Option<i128>> + '_ {
        let mut acc = base;
        (0..self.len()).map(move |k| {
            let e = self.element(k)?;
            if self.increment == Increment::Absolute {
                return Some(e);
            }
            let delta = if self.increment == Increment::Negative { e.checked_neg()? } else { e };
            acc = if self.direction == Direction::Inverse {
                acc.checked_sub(delta)?
            } else {
                acc.checked_add(delta)?
            };
            Some(acc)
        })
    }
}

impl fmt::Display for Profile<'_> {
    /// `profile of 4 increments, every month`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.increment {
            Increment::Absolute => "readings",
            Increment::Positive | Increment::Negative => "increments",
            Increment::Difference => "differences",
        };
        write!(f, "{} {what}", self.len())?;
        match self.spacing {
            Spacing::Every(n, unit) => write!(f, ", every {n} {unit}")?,
            Spacing::HalfMonth => f.write_str(", every half month")?,
            Spacing::Months(1) => f.write_str(", every month")?,
            Spacing::Months(n) => write!(f, ", every {n} months")?,
            Spacing::Columns => f.write_str(", as columns")?,
            Spacing::Reserved(v) => write!(f, ", spacing {v}")?,
        }
        if self.direction == Direction::Inverse {
            f.write_str(", backwards")?;
        }
        Ok(())
    }
}
