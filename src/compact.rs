//! Compact frames (EN 13757-3:2018 Annex G): values without their DIF/VIF layout.
//!
//! A meter sends a full frame now and then (CI `78`/`7A`/`72`) and compact frames (CI
//! `79`/`7B`/`73`) in between. A compact frame carries the **format signature** — a CRC
//! of the full frame's DIF, DIFE, VIF and VIFE bytes — the **full-frame CRC** of the
//! records it stands for, and the data bytes only. A receiver keeps the layouts it has
//! seen, keyed by signature, and rebuilds the full records from them.
//!
//! ```text
//! compact:  79  signature(2, LE)  full-frame CRC(2, LE)  data…
//! format:   69  length  signature(2, LE)  DIF VIF … (the layout only)
//! ```
//!
//! The cache is the caller's: implement [`Formats`] over a map or a database, or use the
//! fixed-size [`FormatCache`] in firmware. [`Format::of`] computes a full frame's layout
//! and signature to put into it.

use crate::crc::crc16;
use crate::record::{DataField, Records};
use crate::{Check, Error, Result};

/// A record layout: the DIF/DIFE/VIF/VIFE bytes of every record, and their signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format<'a> {
    /// CRC-16/EN-13757 of `headers`.
    pub signature: u16,
    /// DIF, DIFEs, VIF and VIFEs of every record, in order; fillers left out.
    pub headers: &'a [u8],
}

impl<'a> Format<'a> {
    /// The layout of `records` (the data records of a full frame), written into `out`.
    /// `None` if a record does not parse, `out` is too small, or a record cannot be
    /// carried in a compact frame (plain-text VIF, manufacturer data).
    pub fn of<'o>(records: &Records<'_>, out: &'o mut [u8]) -> Option<Format<'o>> {
        let mut n = 0usize;
        for r in records.clone() {
            let header = r.ok()?.header?;
            out.get_mut(n..n + header.len())?.copy_from_slice(header);
            n += header.len();
        }
        let headers = out.get(..n)?;
        Some(Format { signature: crc16(headers), headers })
    }

    /// A layout received in a format frame (CI `69`/`6A`/`6B`); `None` if the bytes do
    /// not match the signature they come with.
    pub fn received(signature: u16, headers: &'a [u8]) -> Option<Self> {
        (crc16(headers) == signature).then_some(Self { signature, headers })
    }
}

/// Where compact frames find their layouts.
///
/// **Fill it only from frames you trust at least as much as the compact frames it will
/// serve.** A CRC-16 signature is not a MAC: anyone in radio range can send a format frame
/// or a plaintext full frame whose layout collides with a genuine one and re-reads the
/// same data bytes as other records with a matching full-frame CRC. Expanded records are
/// only as trustworthy as the frame their layout came from: for an authenticated or
/// decrypted compact frame, learn layouts from that meter's decrypted full frames only
/// (004-compact-and-fragments/SR-001).
pub trait Formats {
    /// The DIF/VIF bytes for `signature`, if known.
    fn headers(&self, signature: u16) -> Option<&[u8]>;
}

impl Formats for Format<'_> {
    fn headers(&self, signature: u16) -> Option<&[u8]> {
        (self.signature == signature).then_some(self.headers)
    }
}

impl Formats for [Format<'_>] {
    fn headers(&self, signature: u16) -> Option<&[u8]> {
        self.iter().find_map(|f| f.headers(signature))
    }
}

impl<F: Formats + ?Sized> Formats for &F {
    fn headers(&self, signature: u16) -> Option<&[u8]> {
        (**self).headers(signature)
    }
}

/// A fixed-size cache of `N` layouts of at most `B` bytes, replacing the oldest: enough
/// for the handful of meters a firmware listens to, without an allocator.
#[derive(Clone, Debug)]
pub struct FormatCache<const N: usize, const B: usize> {
    entries: [(u16, u8, [u8; B]); N],
    used: usize,
    next: usize,
}

impl<const N: usize, const B: usize> Default for FormatCache<N, B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const B: usize> FormatCache<N, B> {
    /// An empty cache.
    pub const fn new() -> Self {
        Self { entries: [(0, 0, [0; B]); N], used: 0, next: 0 }
    }

    /// Remember a layout (replacing the oldest when full). `false` if it is longer than
    /// `B` bytes (or 255, the most a format frame can carry).
    pub fn insert(&mut self, format: &Format<'_>) -> bool {
        let len = format.headers.len();
        let Ok(stored) = u8::try_from(len) else { return false };
        if len > B || N == 0 {
            return false;
        }
        if self.headers(format.signature).is_some() {
            return true;
        }
        let Some((sig, l, bytes)) = self.entries.get_mut(self.next) else { return false };
        let Some(dst) = bytes.get_mut(..len) else { return false };
        dst.copy_from_slice(format.headers);
        *sig = format.signature;
        *l = stored;
        self.next = (self.next + 1) % N;
        self.used = (self.used + 1).min(N);
        true
    }
}

impl<const N: usize, const B: usize> Formats for FormatCache<N, B> {
    fn headers(&self, signature: u16) -> Option<&[u8]> {
        self.entries
            .iter()
            .take(self.used)
            .find(|(s, ..)| *s == signature)
            .and_then(|(_, l, b)| b.get(..usize::from(*l)))
    }
}

/// A compact frame's application data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compact<'a> {
    /// The format signature: which layout the data follows.
    pub signature: u16,
    /// CRC-16/EN-13757 of the full records this frame stands for.
    pub crc: u16,
    /// The data bytes of every record, concatenated.
    pub data: &'a [u8],
    /// Came out of a decrypted ELL: the expanded records say so.
    decrypted: bool,
}

impl Compact<'_> {
    pub(crate) fn parse(bytes: &[u8], decrypted: bool) -> Result<Compact<'_>> {
        let (&[s0, s1, c0, c1], data) = bytes.split_first_chunk::<4>().ok_or(Error::UnexpectedEnd)?;
        let (signature, crc) = (u16::from_le_bytes([s0, s1]), u16::from_le_bytes([c0, c1]));
        Ok(Compact { signature, crc, data, decrypted })
    }

    /// Rebuild the full records into `out` with the layout from `formats`, and verify the
    /// full-frame CRC. [`Error::UnknownFormat`] when the layout is not known yet (wait for
    /// a full frame); [`Check::FullFrameCrc`] when the rebuilt records do not match.
    pub fn expand<'o, F: Formats + ?Sized>(&self, formats: &F, out: &'o mut [u8]) -> Result<Records<'o>> {
        let headers = formats.headers(self.signature).ok_or(Error::UnknownFormat(self.signature))?;
        let mut h = 0usize; // position in headers
        let mut d = 0usize; // position in data
        let mut n = 0usize; // position in out
        let mut put = |bytes: &[u8], n: &mut usize| -> Result<()> {
            out.get_mut(*n..*n + bytes.len()).ok_or(Error::OutputTooSmall)?.copy_from_slice(bytes);
            *n += bytes.len();
            Ok(())
        };
        while h < headers.len() {
            let start = h;
            let dif = *headers.get(h).ok_or(Error::UnexpectedEnd)?;
            h += 1;
            if dif & 0x0F == 0x0F {
                // Manufacturer data (0F/1F) and specials are not part of a layout.
                return Err(Error::InvalidFormat);
            }
            let mut ext = dif & 0x80 != 0;
            while ext {
                ext = headers.get(h).ok_or(Error::UnexpectedEnd)? & 0x80 != 0;
                h += 1;
            }
            let vif = *headers.get(h).ok_or(Error::UnexpectedEnd)?;
            h += 1;
            if vif & 0x7F == 0x7C {
                // A plain-text unit's text sits between VIF and data: not in a layout.
                return Err(Error::InvalidFormat);
            }
            let mut ext = vif & 0x80 != 0;
            while ext {
                ext = headers.get(h).ok_or(Error::UnexpectedEnd)? & 0x80 != 0;
                h += 1;
            }
            put(headers.get(start..h).ok_or(Error::UnexpectedEnd)?, &mut n)?;
            let len = match DataField::from_nibble(dif) {
                DataField::None | DataField::Selection => 0,
                DataField::Int(k) | DataField::Bcd(k) => usize::from(k),
                DataField::Real32 => 4,
                DataField::Variable => {
                    1 + crate::record::lvar_len(*self.data.get(d).ok_or(Error::UnexpectedEnd)?)?
                }
            };
            put(self.data.get(d..d + len).ok_or(Error::UnexpectedEnd)?, &mut n)?;
            d += len;
        }
        let full = out.get(..n).ok_or(Error::OutputTooSmall)?;
        if d != self.data.len() || crc16(full) != self.crc {
            return Err(Error::Verification(Check::FullFrameCrc));
        }
        Ok(if self.decrypted { Records::decrypted(full, full.len()) } else { Records::new(full) })
    }
}
