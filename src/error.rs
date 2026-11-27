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
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
