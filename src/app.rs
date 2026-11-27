//! The application layer (EN 13757-3): CI field, data header and payload.
//!
//! | CI | Payload |
//! |---|---|
//! | `72` | long header (12 bytes) + data records |
//! | `7A` | short header (4 bytes) + data records |
//! | `78` | data records, no header |
//! | `51` | data records sent to a slave (`SND_UD`) |
//!
//! A header's configuration field says whether, and how, the records are encrypted.
//! Encrypted payloads are returned as [`Body::Encrypted`]; decryption (OMS security
//! modes 5 and 7) is planned behind an opt-in feature.

use core::fmt;

use crate::record::Records;
use crate::{Error, Result};

/// CI values this crate understands.
pub mod ci {
    /// Response with variable data structure, long header.
    pub const RSP_LONG: u8 = 0x72;
    /// Response with variable data structure, short header.
    pub const RSP_SHORT: u8 = 0x7A;
    /// Response with variable data structure, no header.
    pub const RSP_NONE: u8 = 0x78;
    /// Data send (master to slave), no header.
    pub const SND_UD: u8 = 0x51;
}

/// A manufacturer code: three letters packed into 15 bits (the FLAG association ID).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Manufacturer(pub u16);

impl Manufacturer {
    /// The code for three upper-case letters, e.g. `b"KAM"`.
    pub const fn from_letters(l: [u8; 3]) -> Option<Self> {
        let [a, b, c] = l;
        if !a.is_ascii_uppercase() || !b.is_ascii_uppercase() || !c.is_ascii_uppercase() {
            return None;
        }
        Some(Self((((a - 64) as u16) << 10) | (((b - 64) as u16) << 5) | ((c - 64) as u16)))
    }

    /// The three letters.
    pub const fn letters(self) -> [u8; 3] {
        let v = self.0;
        [(((v >> 10) & 0x1F) as u8) + 64, (((v >> 5) & 0x1F) as u8) + 64, ((v & 0x1F) as u8) + 64]
    }
}

impl fmt::Display for Manufacturer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.letters() {
            write!(f, "{}", char::from(b))?;
        }
        Ok(())
    }
}

/// The medium (device type) a meter measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Medium(pub u8);

impl Medium {
    /// Electricity.
    pub const ELECTRICITY: Self = Self(0x02);
    /// Gas.
    pub const GAS: Self = Self(0x03);
    /// Heat, measured at the outlet.
    pub const HEAT: Self = Self(0x04);
    /// Warm water (30–90 °C).
    pub const WARM_WATER: Self = Self(0x06);
    /// Water.
    pub const WATER: Self = Self(0x07);
    /// Heat cost allocator.
    pub const HEAT_COST_ALLOCATOR: Self = Self(0x08);
    /// Cooling load, measured at the outlet.
    pub const COOLING: Self = Self(0x0A);
    /// Heat, measured at the inlet.
    pub const HEAT_INLET: Self = Self(0x0C);
    /// Combined heat and cooling.
    pub const HEAT_COOLING: Self = Self(0x0D);
    /// Hot water (≥ 90 °C).
    pub const HOT_WATER: Self = Self(0x15);
    /// Cold water.
    pub const COLD_WATER: Self = Self(0x16);

    /// A short English name, if this crate knows the code.
    pub const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0x00 => "other",
            0x01 => "oil",
            0x02 => "electricity",
            0x03 => "gas",
            0x04 => "heat (outlet)",
            0x05 => "steam",
            0x06 => "warm water",
            0x07 => "water",
            0x08 => "heat cost allocator",
            0x09 => "compressed air",
            0x0A => "cooling (outlet)",
            0x0B => "cooling (inlet)",
            0x0C => "heat (inlet)",
            0x0D => "heat / cooling",
            0x0E => "bus / system",
            0x0F => "unknown",
            0x15 => "hot water",
            0x16 => "cold water",
            0x17 => "dual water",
            0x18 => "pressure",
            0x19 => "A/D converter",
            0x1A => "smoke detector",
            0x1B => "room sensor",
            0x1C => "gas detector",
            _ => return None,
        })
    }
}

impl fmt::Display for Medium {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => f.write_str(n),
            None => write!(f, "medium 0x{:02x}", self.0),
        }
    }
}

/// The security mode from a header's configuration field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityMode {
    /// The mode: 0 = none, 5 = AES-128-CBC with a static key (OMS), 7 = AES-128-CBC
    /// with an ephemeral key and CMAC (OMS 4), others as defined in EN 13757-7.
    pub mode: u8,
    /// For modes 5 and 7: the number of encrypted 16-byte blocks.
    pub encrypted_blocks: u8,
}

impl SecurityMode {
    /// Read the mode from the 2-byte configuration field (as transmitted, LSB first).
    pub const fn from_config(config: [u8; 2]) -> Self {
        let [low, high] = config;
        Self { mode: high & 0x1F, encrypted_blocks: low >> 4 }
    }
}

/// The short header (CI `7A`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShortHeader {
    /// Access number, incremented by the meter per telegram.
    pub access_number: u8,
    /// Status byte (application errors, battery low, …).
    pub status: u8,
    /// Configuration field, as transmitted.
    pub config: [u8; 2],
}

impl ShortHeader {
    /// The security mode the configuration field announces.
    pub const fn security(&self) -> SecurityMode {
        SecurityMode::from_config(self.config)
    }
}

/// The long header (CI `72`): the short header plus the meter's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongHeader {
    /// Identification number, 8 BCD digits, as transmitted (LSB first).
    pub id: [u8; 4],
    /// Manufacturer.
    pub manufacturer: Manufacturer,
    /// Version / generation.
    pub version: u8,
    /// Medium.
    pub medium: Medium,
    /// Access number, status and configuration.
    pub short: ShortHeader,
}

impl LongHeader {
    /// The identification number as printed on the meter, if it is valid BCD.
    pub fn id_number(&self) -> Option<u32> {
        crate::value::bcd_u32(self.id)
    }
}

/// The data header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    /// No header (CI `78`, `51`).
    None,
    /// Short header (CI `7A`).
    Short(ShortHeader),
    /// Long header (CI `72`).
    Long(LongHeader),
}

impl Header {
    /// The security mode, or mode 0 when there is no header.
    pub const fn security(&self) -> SecurityMode {
        match self {
            Self::None => SecurityMode { mode: 0, encrypted_blocks: 0 },
            Self::Short(s) => s.security(),
            Self::Long(l) => l.short.security(),
        }
    }
}

/// The payload after the header.
#[derive(Clone, Debug)]
pub enum Body<'a> {
    /// Plain data records.
    Records(Records<'a>),
    /// Encrypted data, not decrypted by this version.
    Encrypted {
        /// The security mode.
        security: SecurityMode,
        /// The encrypted bytes (and any plain bytes after them).
        data: &'a [u8],
    },
}

/// A parsed application layer: header and body.
#[derive(Clone, Debug)]
pub struct Application<'a> {
    /// The CI field.
    pub ci: u8,
    /// The data header.
    pub header: Header,
    /// The payload.
    pub body: Body<'a>,
}

impl<'a> Application<'a> {
    /// Parse the bytes after a CI field.
    pub fn parse(ci: u8, data: &'a [u8]) -> Result<Self> {
        let (header, rest) = match ci {
            ci::RSP_LONG => {
                let (h, rest) = data.split_first_chunk::<12>().ok_or(Error::UnexpectedEnd)?;
                let [i0, i1, i2, i3, m0, m1, version, medium, access, status, c0, c1] = *h;
                let header = Header::Long(LongHeader {
                    id: [i0, i1, i2, i3],
                    manufacturer: Manufacturer(u16::from_le_bytes([m0, m1])),
                    version,
                    medium: Medium(medium),
                    short: ShortHeader { access_number: access, status, config: [c0, c1] },
                });
                (header, rest)
            }
            ci::RSP_SHORT => {
                let (h, rest) = data.split_first_chunk::<4>().ok_or(Error::UnexpectedEnd)?;
                let [access, status, c0, c1] = *h;
                (Header::Short(ShortHeader { access_number: access, status, config: [c0, c1] }), rest)
            }
            ci::RSP_NONE | ci::SND_UD => (Header::None, data),
            other => return Err(Error::UnsupportedCi(other)),
        };
        let security = header.security();
        let body = if security.mode == 0 {
            Body::Records(Records::new(rest))
        } else {
            Body::Encrypted { security, data: rest }
        };
        Ok(Self { ci, header, body })
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::string::ToString;

    #[test]
    fn manufacturer_codes() {
        assert_eq!(Manufacturer(0x4024).to_string(), "PAD");
        let kam = Manufacturer::from_letters(*b"KAM").unwrap();
        assert_eq!(kam.0, 0x2C2D);
        assert_eq!(kam.to_string(), "KAM");
        assert_eq!(Manufacturer::from_letters(*b"k1m"), None);
    }

    #[test]
    fn security_mode_from_config() {
        // OMS mode 5 with 2 encrypted blocks: configuration 0x0520, sent as 20 05.
        let s = SecurityMode::from_config([0x20, 0x05]);
        assert_eq!(s, SecurityMode { mode: 5, encrypted_blocks: 2 });
        let app = Application::parse(ci::RSP_SHORT, &[0x01, 0x00, 0x20, 0x05, 0xAA, 0xBB]).unwrap();
        assert!(
            matches!(app.body, Body::Encrypted { security, data } if security.mode == 5 && data == [0xAA, 0xBB])
        );
    }

    #[test]
    fn unsupported_ci() {
        assert_eq!(Application::parse(0x8C, &[]).unwrap_err(), Error::UnsupportedCi(0x8C));
    }
}
