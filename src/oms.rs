//! OMS security (feature `oms`): keys and the AES primitives of security modes 5 and 7
//! and the ELL (OMS Vol. 2, EN 13757-7).
//!
//! | Layer | Cipher | Verified by |
//! |---|---|---|
//! | TPL, mode 5 | AES-128-CBC, IV = M ‖ A ‖ 8 × access number | `2F 2F` after decryption |
//! | TPL, mode 7 | AES-128-CBC, IV = 0, key derived per message (CMAC KDF) | AFL CMAC (and `2F 2F`) |
//! | TPL, mode 10 | AES-128-CCM, nonce = M ‖ A ‖ 00 ‖ counter, key derived per message | CCM tag (authenticates) |
//! | ELL (CI `8D`/`8F`) | AES-128-CTR, IV = M ‖ A ‖ CC ‖ SN ‖ FN ‖ BC | payload CRC |
//!
//! Keys are the caller's: [`Key`] holds one, erases it on drop and never shows it in
//! `Debug`. [`Keys`] is how the library asks for the key of a meter; implement it for a
//! key store, or use a single [`Key`] or a slice of `(Address, Key)` pairs.
//!
//! Only mode 7 with a verified CMAC authenticates a telegram. Mode 5 and the ELL give
//! confidentiality only: `2F 2F` and the payload CRC detect a wrong key, not tampering.
//! Both are malleable without the key (CBC through the IV inputs in the header, CTR with
//! a recomputed CRC), and a fleet-wide key lets its holders send anything
//! ([`Authenticity`](crate::app::Authenticity)).

use core::fmt;

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecryptMut, KeyInit, KeyIvInit, StreamCipher};
use cmac::{Cmac, Mac};
use zeroize::Zeroize;

use crate::app::Address;
use crate::{Check, Error, Result};

/// A 128-bit AES key. Erased from memory when dropped; never printed.
#[derive(Clone)]
pub struct Key([u8; 16]);

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Key {
    /// A key from its 16 bytes.
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// A key from 32 hex digits (whitespace allowed), as meter key sheets print them.
    /// `None` if the text is not exactly 16 bytes of hex. The intermediate buffer is
    /// erased.
    pub fn from_hex(text: &str) -> Option<Self> {
        let mut key = Self([0; 16]);
        let mut n = 0usize;
        let mut high: Option<u8> = None;
        for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
            let v = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return None,
            };
            match high.take() {
                None => high = Some(v),
                Some(h) => {
                    *key.0.get_mut(n)? = (h << 4) | v;
                    n += 1;
                }
            }
        }
        (n == 16 && high.is_none()).then_some(key)
    }

    pub(crate) const fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for Key {
    /// `Key(..)`: the bytes never appear (constitution IV).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key(..)")
    }
}

impl From<[u8; 16]> for Key {
    fn from(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

/// Where the library gets the key for a meter.
pub trait Keys {
    /// The key for `meter`, or `None`.
    fn key(&self, meter: &Address) -> Option<&Key>;
}

/// One key for every meter: for a single meter, or a test.
impl Keys for Key {
    fn key(&self, _: &Address) -> Option<&Key> {
        Some(self)
    }
}

/// A key per meter, matched on manufacturer and identification number (version and
/// device type are not part of a key sheet, and a gateway's long header may differ).
impl Keys for [(Address, Key)] {
    fn key(&self, meter: &Address) -> Option<&Key> {
        self.iter().find(|(a, _)| a.manufacturer == meter.manufacturer && a.id == meter.id).map(|(_, k)| k)
    }
}

impl<const N: usize> Keys for [(Address, Key); N] {
    fn key(&self, meter: &Address) -> Option<&Key> {
        self.as_slice().key(meter)
    }
}

impl<K: Keys + ?Sized> Keys for &K {
    fn key(&self, meter: &Address) -> Option<&Key> {
        (**self).key(meter)
    }
}

/// AES-128-CBC decryption of whole blocks, in place.
pub(crate) fn cbc_decrypt(key: &Key, iv: &[u8; 16], data: &mut [u8]) -> Result<()> {
    if data.len() % 16 != 0 {
        return Err(Error::Verification(Check::Length));
    }
    let mut cipher = cbc::Decryptor::<Aes128>::new(key.bytes().into(), iv.into());
    for block in data.chunks_exact_mut(16) {
        cipher.decrypt_block_mut(GenericArray::from_mut_slice(block));
    }
    Ok(())
}

/// AES-128-CCM decryption in place with tag verification (8-byte tags, as OMS requires).
/// The plaintext is only left in `data` when the tag verifies; otherwise it is erased.
pub(crate) fn ccm_decrypt(
    key: &Key,
    nonce: &[u8; 13],
    aad: &[u8],
    data: &mut [u8],
    tag: &[u8],
) -> Result<()> {
    use ccm::aead::AeadInPlace;
    use ccm::consts::{U8, U13};
    let tag: &[u8; 8] = tag.try_into().map_err(|_| Error::UnsupportedSecurity(10))?;
    let cipher = ccm::Ccm::<Aes128, U8, U13>::new(key.bytes().into());
    cipher.decrypt_in_place_detached(nonce.into(), aad, data, tag.into()).map_err(|_| {
        data.zeroize();
        Error::Verification(Check::Tag)
    })
}

/// AES-128-CTR (big-endian 128-bit counter), in place.
pub(crate) fn ctr_apply(key: &Key, iv: &[u8; 16], data: &mut [u8]) {
    ctr::Ctr128BE::<Aes128>::new(key.bytes().into(), iv.into()).apply_keystream(data);
}

/// AES-CMAC over the concatenation of `parts`.
pub(crate) fn cmac(key: &Key, parts: &[&[u8]]) -> [u8; 16] {
    let mut mac = <Cmac<Aes128> as KeyInit>::new(key.bytes().into());
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// Constant-time comparison of a received MAC against the computed one.
pub(crate) fn mac_matches(computed: &[u8], received: &[u8]) -> bool {
    computed.len() == received.len()
        && computed.iter().zip(received).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The OMS key derivation (OMS Vol. 2 §9.5, KDF-A): `CMAC(K, D ‖ C ‖ ID ‖ 07…07)`, with
/// `D = 00` for the encryption key from the meter, `01` for its MAC key, the message
/// counter `C` and the meter's identification number, both as transmitted.
pub(crate) fn derive(key: &Key, d: u8, counter: [u8; 4], id: [u8; 4]) -> Key {
    let [c0, c1, c2, c3] = counter;
    let [i0, i1, i2, i3] = id;
    let input = [d, c0, c1, c2, c3, i0, i1, i2, i3, 7, 7, 7, 7, 7, 7, 7];
    let mut derived = cmac(key, &[&input]);
    let k = Key(derived);
    derived.zeroize();
    k
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::format;

    #[test]
    fn keys_never_print() {
        let k = Key::from_hex("01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F 11").unwrap();
        assert_eq!(format!("{k:?}"), "Key(..)");
        assert_eq!(k.bytes()[15], 0x11);
        assert!(Key::from_hex("0102").is_none());
        assert!(Key::from_hex("zz02030405060708090A0B0C0D0E0F11").is_none());
        assert!(Key::from_hex("0102030405060708090A0B0C0D0E0F1122").is_none());
    }

    #[test]
    fn cmac_matches_rfc_4493() {
        // RFC 4493, example 2: K = 2b7e1516…, M = 6bc1bee2 2e409f96 e93d7e11 7393172a.
        let k = Key::from_hex("2b7e151628aed2a6abf7158809cf4f3c").unwrap();
        let m =
            [0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a];
        let t = cmac(&k, &[&m[..8], &m[8..]]);
        assert_eq!(
            t,
            [0x07, 0x0a, 0x16, 0xb4, 0x6b, 0x4d, 0x41, 0x44, 0xf7, 0x9b, 0xdd, 0x9d, 0xd0, 0x4a, 0x28, 0x7c]
        );
    }
}
