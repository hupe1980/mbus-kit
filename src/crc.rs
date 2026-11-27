//! CRC-16/EN-13757, the block checksum of wireless M-Bus (EN 13757-4).
//!
//! Polynomial 0x3D65, initial value 0x0000, not reflected, final XOR 0xFFFF,
//! transmitted most significant byte first. The standard check value over
//! `"123456789"` is `0xC2B7`.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_check_value() {
        assert_eq!(crc16(b"123456789"), 0xC2B7);
    }
}
