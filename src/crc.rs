//! The CRCs of wireless M-Bus and its receiver sticks.
//!
//! [`crc16`] is CRC-16/EN-13757, the block checksum of wireless M-Bus (EN 13757-4):
//! polynomial 0x3D65, initial value 0x0000, not reflected, final XOR 0xFFFF, transmitted
//! most significant byte first; check value over `"123456789"` `0xC2B7`.
//!
//! [`crc16_x25`] is CRC-16/X-25 (IBM-SDLC), the frame check of the IMST sticks' host
//! controller interfaces: reflected polynomial 0x8408, initial value 0xFFFF, final XOR
//! 0xFFFF, transmitted least significant byte first; check value `0x906E`.

const fn table() -> [u16; 256] {
    let mut t = [0u16; 256];
    let mut i = 0usize;
    while i < 256 {
        #[allow(clippy::cast_possible_truncation)] // i < 256
        let mut crc = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x3D65 } else { crc << 1 };
            bit += 1;
        }
        #[allow(clippy::indexing_slicing)] // i < 256, and a const fn cannot use get_mut
        {
            t[i] = crc;
        }
        i += 1;
    }
    t
}

static TABLE: [u16; 256] = table();

/// The CRC-16/EN-13757 of `bytes`.
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &b in bytes {
        let [high, _] = crc.to_be_bytes();
        let idx = usize::from(high ^ b);
        crc = (crc << 8) ^ TABLE.get(idx).copied().unwrap_or(0);
    }
    crc ^ 0xFFFF
}

/// The CRC-16/X-25 of `bytes` (bitwise: the stick frames are short).
pub fn crc16_x25(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &b in bytes {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x8408 } else { crc >> 1 };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_check_value() {
        assert_eq!(crc16(b"123456789"), 0xC2B7);
        assert_eq!(crc16_x25(b"123456789"), 0x906E);
    }
}
