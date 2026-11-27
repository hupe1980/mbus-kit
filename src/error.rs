use core::fmt;

/// Result type of this crate.
pub type Result<T> = core::result::Result<T, Error>;

/// Everything that can go wrong between the meter's bytes and a typed record.
///
/// Every variant is a property of the input, never of this crate's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ended in the middle of a frame or record.
    UnexpectedEnd,
    /// The first byte is not a known frame start (`E5`, `10`, `68`).
    InvalidStart(u8),
    /// The two length bytes of a long frame differ, or the second `68` is missing.
    InvalidHeader,
    /// The stop byte (`16`) is missing.
    MissingStop,
    /// The wired checksum does not match.
    Checksum {
        /// Checksum computed over the frame.
        computed: u8,
        /// Checksum on the wire.
        found: u8,
    },
    /// A wireless block CRC does not match.
    Crc {
        /// The block (0-based) whose CRC failed.
        block: usize,
        /// CRC computed over the block.
        computed: u16,
        /// CRC on the wire.
        found: u16,
    },
    /// A length field is inconsistent with the frame.
    InvalidLength,
    /// More than 10 DIFE or VIFE bytes follow one another.
    TooManyExtensions,
    /// A reserved special-function DIF (`3F`…`6F`).
    ReservedDif(u8),
    /// An LVAR value in a reserved range.
    UnsupportedLvar(u8),
    /// The CI field announces something this crate does not parse yet.
    UnsupportedCi(u8),
    /// The output buffer is too small for what is being written.
    OutputTooSmall,
    /// A layer appears where it may not (e.g. a second ELL, or an AFL after the TPL).
    UnexpectedLayer(u8),
    /// The telegram is encrypted with a security mode this crate does not decrypt.
    UnsupportedSecurity(u8),
    /// No key for this meter in the key lookup.
    NoKey,
    /// A compact frame whose layout (format signature) is not known yet: wait for a full
    /// frame of this meter and remember its [`Format`](crate::compact::Format).
    UnknownFormat(u16),
    /// A compact-frame layout that cannot describe values: a special DIF (manufacturer
    /// data, filler) or a plain-text unit in it.
    InvalidFormat,
    /// A fragment that does not continue the message in progress: missing, repeated or
    /// out of order (`found` is its fragment ID; 0 for a telegram that is not a fragment).
    UnexpectedFragment {
        /// The fragment ID expected next (1 when no message is open).
        expected: u8,
        /// The fragment ID received.
        found: u8,
    },
    /// A fragment from another meter than the message in progress.
    FragmentFromOtherMeter,
    /// Decryption or authentication failed: the check that failed, never the key.
    Verification(Check),
    /// A wired primary address no slave answers from: 251 and 252 are reserved, 255 is
    /// the broadcast without reply.
    InvalidAddress(u8),
}

/// Which cryptographic check failed. Says nothing about the key (constitution IV).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Check {
    /// The decrypted payload does not start with `2F 2F` (wrong key, or tampered).
    Marker,
    /// The AFL message authentication code does not match, or the message control
    /// announces one that is missing.
    Mac,
    /// The ELL payload CRC over the decrypted payload does not match.
    PayloadCrc,
    /// The encrypted part is not a whole number of 16-byte blocks, or shorter than
    /// the configuration field announces.
    Length,
    /// Security mode 7 without the AFL message counter the key derivation needs.
    MissingCounter,
    /// The records rebuilt from a compact frame do not match its full-frame CRC.
    FullFrameCrc,
    /// The AES-CCM tag of a mode-10 telegram does not match.
    Tag,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Marker => "decrypted payload does not start with 2F 2F",
            Self::Mac => "message authentication code mismatch",
            Self::PayloadCrc => "ELL payload CRC mismatch",
            Self::Length => "encrypted length is not a whole number of blocks, or longer than the data",
            Self::MissingCounter => "mode 7 without AFL message counter",
            Self::FullFrameCrc => "compact frame does not match its full-frame CRC",
            Self::Tag => "AES-CCM tag mismatch",
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd => f.write_str("input ended in the middle of a value"),
            Self::InvalidStart(b) => write!(f, "0x{b:02x} is not a frame start"),
            Self::InvalidHeader => f.write_str("malformed long-frame header"),
            Self::MissingStop => f.write_str("stop byte 0x16 missing"),
            Self::Checksum { computed, found } => {
                write!(f, "checksum 0x{found:02x} does not match computed 0x{computed:02x}")
            }
            Self::Crc { block, computed, found } => {
                write!(f, "CRC of block {block} is 0x{found:04x}, computed 0x{computed:04x}")
            }
            Self::InvalidLength => f.write_str("length field inconsistent with the frame"),
            Self::TooManyExtensions => f.write_str("more than 10 extension bytes"),
            Self::ReservedDif(d) => write!(f, "reserved DIF 0x{d:02x}"),
            Self::UnsupportedLvar(l) => write!(f, "LVAR 0x{l:02x} is reserved or unsupported"),
            Self::UnsupportedCi(c) => write!(f, "CI field 0x{c:02x} is not supported yet"),
            Self::OutputTooSmall => f.write_str("output buffer too small"),
            Self::UnexpectedLayer(c) => {
                write!(f, "CI 0x{c:02x} is not allowed at this point of the telegram")
            }
            Self::UnsupportedSecurity(m) => write!(f, "security mode {m} is not supported"),
            Self::NoKey => f.write_str("no key for this meter"),
            Self::UnknownFormat(s) => write!(f, "compact frame with unknown format signature 0x{s:04x}"),
            Self::InvalidFormat => f.write_str("compact-frame layout with a special DIF or plain-text unit"),
            Self::UnexpectedFragment { expected, found: 0 } => {
                write!(f, "not a fragment (expected fragment {expected})")
            }
            Self::UnexpectedFragment { expected, found } => {
                write!(f, "fragment {found} where fragment {expected} was expected")
            }
            Self::FragmentFromOtherMeter => f.write_str("fragment from another meter"),
            Self::Verification(c) => write!(f, "verification failed: {c}"),
            Self::InvalidAddress(a) => write!(f, "no meter answers from primary address {a}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
