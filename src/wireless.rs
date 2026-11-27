//! Wireless M-Bus link layer (EN 13757-4): frame formats A and B.
//!
//! Both formats start with the same 10 bytes:
//!
//! ```text
//! L  C  M M  A A A A A A
//!       ^^^  ^^^^^^^^^^^ address: ID (4, BCD, LSB first), version, device type
//!       manufacturer (LE, three letters)
//! ```
//!
//! **Format A** (modes S, T and C with preamble A): every block carries its own CRC.
//! Block 1 is those 10 bytes, then blocks of 16 bytes, the last one shorter. `L` counts
//! the bytes after itself **without** the CRCs.
//!
//! **Format B** (mode C with preamble B): block 1 has no CRC; block 2 (up to 118 bytes
//! including its CRC) ends with a CRC over blocks 1 and 2; a longer frame has a third
//! block with its own CRC. `L` counts the bytes after itself **with** the CRCs.
//!
//! CRCs are CRC-16/EN-13757, most significant byte first. The decoder copies the data
//! without CRCs into a caller buffer, so the frame borrows from that buffer.

use crate::app::{Manufacturer, Medium};
use crate::crc::crc16;
use crate::{Error, Result};

/// Frame format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Format A: a CRC after every block.
    A,
    /// Format B: CRCs after block 2 and block 3 only.
    B,
}

/// C-field values a meter sends.
pub mod control {
    /// `SND_NR`: unsolicited user data, no reply expected. A meter's periodic telegram.
    pub const SND_NR: u8 = 0x44;
    /// `SND_IR`: installation request.
    pub const SND_IR: u8 = 0x46;
    /// `ACC_NR`: access demand, no data.
    pub const ACC_NR: u8 = 0x47;
    /// `RSP_UD`: response to a request.
    pub const RSP_UD: u8 = 0x08;
}

/// The address of a wireless meter: manufacturer, ID, version and device type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Address {
    /// Manufacturer (FLAG association code).
    pub manufacturer: Manufacturer,
    /// Identification number, 8 BCD digits, as transmitted (LSB first).
    pub id: [u8; 4],
    /// Version / generation.
    pub version: u8,
    /// Device type (medium).
    pub medium: Medium,
}

impl Address {
    /// The identification number as the decimal printed on the meter, if it is valid BCD.
    pub fn id_number(&self) -> Option<u32> {
        crate::value::bcd_u32(self.id)
    }

    fn bytes(self) -> [u8; 8] {
        let [m0, m1] = self.manufacturer.0.to_le_bytes();
        let [i0, i1, i2, i3] = self.id;
        [m0, m1, i0, i1, i2, i3, self.version, self.medium.0]
    }
}

/// A decoded wireless frame, CRCs removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'o> {
    /// C field.
    pub c: u8,
    /// The sending meter.
    pub address: Address,
    /// CI field: what the payload is.
    pub ci: u8,
    /// The payload after the CI field (header and records, possibly encrypted).
    pub payload: &'o [u8],
}

/// Number of bytes on the air for a frame whose `L` field is `l`, in `format`.
pub fn wire_len(l: u8, format: Format) -> usize {
    let l = usize::from(l);
    match format {
        Format::A => {
            let rest = l.saturating_sub(9);
            1 + l + 2 + 2 * rest.div_ceil(16)
        }
        Format::B => 1 + l,
    }
}

/// Decode a frame from `raw` (starting at the `L` field), checking every CRC. The data
/// without CRCs is copied into `out`, which must hold at least `L + 1` bytes.
///
/// Returns `Ok(None)` when `raw` is shorter than the frame its `L` field announces.
pub fn decode<'o>(raw: &[u8], format: Format, out: &'o mut [u8]) -> Result<Option<(Frame<'o>, usize)>> {
    let Some(&l) = raw.first() else { return Ok(None) };
    if l < 10 {
        return Err(Error::InvalidLength);
    }
    let total = wire_len(l, format);
    let Some(raw) = raw.get(..total) else { return Ok(None) };
    let len = match format {
        Format::A => strip_a(raw, out)?,
        Format::B => strip_b(raw, out)?,
    };
    let data = out.get(..len).ok_or(Error::OutputTooSmall)?;
    let frame = parse_fields(data)?;
    Ok(Some((frame, total)))
}

fn copy_into(out: &mut [u8], at: usize, bytes: &[u8]) -> Result<usize> {
    let end = at.checked_add(bytes.len()).ok_or(Error::OutputTooSmall)?;
    out.get_mut(at..end).ok_or(Error::OutputTooSmall)?.copy_from_slice(bytes);
    Ok(end)
}

fn check_crc(block: usize, data: &[u8], crc: &[u8]) -> Result<()> {
    let computed = crc16(data);
    let found = match crc {
        [hi, lo] => u16::from_be_bytes([*hi, *lo]),
        _ => return Err(Error::UnexpectedEnd),
    };
    if computed == found { Ok(()) } else { Err(Error::Crc { block, computed, found }) }
}

fn strip_a(raw: &[u8], out: &mut [u8]) -> Result<usize> {
    let mut pos = 0usize;
    let mut written = 0usize;
    let mut block = 0usize;
    let mut size = 10usize;
    while pos < raw.len() {
        let data_end = (pos + size).min(raw.len().saturating_sub(2));
        let data = raw.get(pos..data_end).ok_or(Error::UnexpectedEnd)?;
        let crc = raw.get(data_end..data_end + 2).ok_or(Error::UnexpectedEnd)?;
        check_crc(block, data, crc)?;
        written = copy_into(out, written, data)?;
        pos = data_end + 2;
        block += 1;
        size = 16;
    }
    Ok(written)
}

fn strip_b(raw: &[u8], out: &mut [u8]) -> Result<usize> {
    // Block 1 (10 bytes) and block 2 (up to 118 bytes with CRC) share the first CRC.
    let first_end = raw.len().min(128);
    let first = raw.get(..first_end.saturating_sub(2)).ok_or(Error::UnexpectedEnd)?;
    check_crc(0, first, raw.get(first.len()..first_end).ok_or(Error::UnexpectedEnd)?)?;
    let mut written = copy_into(out, 0, first)?;
    if raw.len() > 128 {
        let third = raw.get(128..raw.len() - 2).ok_or(Error::UnexpectedEnd)?;
        check_crc(1, third, raw.get(raw.len() - 2..).ok_or(Error::UnexpectedEnd)?)?;
        written = copy_into(out, written, third)?;
    }
    Ok(written)
}

fn parse_fields(data: &[u8]) -> Result<Frame<'_>> {
    let Some((&[_l, c, m0, m1, i0, i1, i2, i3, version, medium, ci], payload)) =
        data.split_first_chunk::<11>()
    else {
        return Err(Error::UnexpectedEnd);
    };
    Ok(Frame {
        c,
        address: Address {
            manufacturer: Manufacturer(u16::from_le_bytes([m0, m1])),
            id: [i0, i1, i2, i3],
            version,
            medium: Medium(medium),
        },
        ci,
        payload,
    })
}

/// Encode a frame in format A: link fields, CI and payload, with a CRC after every
/// block. Returns the number of bytes written. For simulators and tests.
pub fn encode_a(c: u8, address: &Address, ci: u8, payload: &[u8], out: &mut [u8]) -> Result<usize> {
    let l = payload.len().checked_add(10).filter(|&l| l <= 255).ok_or(Error::InvalidLength)?;
    let l8 = u8::try_from(l).map_err(|_| Error::InvalidLength)?;
    // The CRC-free data: L, C, address (8), CI, payload.
    let mut head = [0u8; 11];
    head[0] = l8;
    head[1] = c;
    head[2..10].copy_from_slice(&(*address).bytes());
    head[10] = ci;
    let data = head.iter().copied().chain(payload.iter().copied());

    let mut written = 0usize;
    let mut block = [0u8; 16];
    let mut in_block = 0usize;
    let mut size = 10usize;
    let flush = |block: &[u8], written: &mut usize, out: &mut [u8]| -> Result<()> {
        *written = copy_into(out, *written, block)?;
        *written = copy_into(out, *written, &crc16(block).to_be_bytes())?;
        Ok(())
    };
    for b in data {
        *block.get_mut(in_block).ok_or(Error::InvalidLength)? = b;
        in_block += 1;
        if in_block == size {
            flush(block.get(..in_block).unwrap_or(&[]), &mut written, out)?;
            in_block = 0;
            size = 16;
        }
    }
    if in_block > 0 {
        flush(block.get(..in_block).unwrap_or(&[]), &mut written, out)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> Address {
        Address {
            manufacturer: Manufacturer::from_letters(*b"KAM").unwrap(),
            id: [0x78, 0x56, 0x34, 0x12],
            version: 0x1B,
            medium: Medium::WATER,
        }
    }

    #[test]
    fn format_a_round_trip_for_every_length() {
        for n in 0..60usize {
            let payload: [u8; 60] = core::array::from_fn(|i| u8::try_from(i).unwrap() ^ 0x5A);
            let mut wire = [0u8; 128];
            let w = encode_a(control::SND_NR, &addr(), 0x7A, &payload[..n], &mut wire).unwrap();
            assert_eq!(w, wire_len(wire[0], Format::A));
            let mut out = [0u8; 128];
            let (frame, used) = decode(&wire[..w], Format::A, &mut out).unwrap().unwrap();
            assert_eq!(used, w);
            assert_eq!(frame.c, control::SND_NR);
            assert_eq!(frame.address, addr());
            assert_eq!(frame.address.id_number(), Some(12_345_678));
            assert_eq!(frame.ci, 0x7A);
            assert_eq!(frame.payload, &payload[..n]);
        }
    }

    #[test]
    fn every_flipped_byte_is_caught() {
        let mut wire = [0u8; 64];
        let w =
            encode_a(control::SND_NR, &addr(), 0x7A, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13], &mut wire)
                .unwrap();
        for i in 1..w {
            let mut bad = wire;
            bad[i] ^= 0x10;
            let mut out = [0u8; 64];
            assert!(decode(&bad[..w], Format::A, &mut out).is_err(), "byte {i}");
        }
    }

    #[test]
    fn short_input_asks_for_more() {
        let mut wire = [0u8; 64];
        let w = encode_a(control::SND_NR, &addr(), 0x7A, &[0; 20], &mut wire).unwrap();
        let mut out = [0u8; 64];
        assert_eq!(decode(&wire[..w - 1], Format::A, &mut out).unwrap(), None);
    }

    #[test]
    fn format_b_single_block() {
        // Build format B by hand: L counts the CRC; one CRC over everything before it.
        let a = addr().bytes();
        let mut data = [0u8; 15];
        data[0] = 14; // 1 C + 8 address + 1 CI + 2 payload + 2 CRC
        data[1] = control::SND_NR;
        data[2..10].copy_from_slice(&a);
        data[10] = 0x78;
        data[11] = 0x2F;
        data[12] = 0x2F;
        let crc = crc16(&data[..13]).to_be_bytes();
        data[13..15].copy_from_slice(&crc);
        let mut out = [0u8; 32];
        let (frame, used) = decode(&data, Format::B, &mut out).unwrap().unwrap();
        assert_eq!(used, 15);
        assert_eq!(frame.payload, &[0x2F, 0x2F]);
    }
}
