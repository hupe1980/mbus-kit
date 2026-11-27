//! The layers above the link layer: ELL, AFL and TPL (EN 13757-3, EN 13757-7,
//! OMS Vol. 2), and the data records they carry.
//!
//! ```text
//! [ELL 8C–8F]  [AFL 90]  TPL (CI + header)  records
//! ```
//!
//! | CI | Layer | Header |
//! |---|---|---|
//! | `8C`, `8E` | extended link layer: CC, access number (`8E`: + receiver address) | |
//! | `8D`, `8F` | the same + session number and payload CRC; **AES-CTR** encrypted | |
//! | `90` | authentication and fragmentation layer: message counter, **CMAC** | |
//! | `72`, `5B`, `6F`, `75`, `8B`, `80` | transport layer | long (12 bytes) |
//! | `7A`, `5A`, `6E`, `74`, `8A` | transport layer | short (4 bytes) |
//! | `78`, `51`, `70`, `71` | transport layer | none |
//!
//! [`Application::parse`] reads every layer without a key and stops at the first
//! encrypted one: [`Body::Encrypted`] then says how it is encrypted, and (feature `oms`)
//! [`Encrypted::decrypt`] opens it. [`Application::authenticity`] says what, if
//! anything, was cryptographically verified: radio data is not evidence until it is
//! (constitution V).

use core::fmt;

use crate::crc::crc16;
use crate::record::Records;
use crate::{Check, Error, Result};

/// CI values.
pub mod ci {
    /// Response with variable data structure, long header.
    pub const RSP_LONG: u8 = 0x72;
    /// Response with variable data structure, short header.
    pub const RSP_SHORT: u8 = 0x7A;
    /// Response with variable data structure, no header.
    pub const RSP_NONE: u8 = 0x78;
    /// Data send (master to slave), no header.
    pub const SND_UD: u8 = 0x51;
    /// Command to a device, short header (OMS).
    pub const CMD_SHORT: u8 = 0x5A;
    /// Command to a device, long header (OMS).
    pub const CMD_LONG: u8 = 0x5B;
    /// Application error, short header.
    pub const ERROR_SHORT: u8 = 0x6E;
    /// Application error, long header.
    pub const ERROR_LONG: u8 = 0x6F;
    /// Application error, no header.
    pub const ERROR_NONE: u8 = 0x70;
    /// Alarm, no header.
    pub const ALARM_NONE: u8 = 0x71;
    /// Alarm, short header.
    pub const ALARM_SHORT: u8 = 0x74;
    /// Alarm, long header.
    pub const ALARM_LONG: u8 = 0x75;
    /// Transport layer to a device, long header, no application data.
    pub const TPL_TO_DEVICE: u8 = 0x80;
    /// Transport layer from a device, short header, no application data.
    pub const TPL_SHORT: u8 = 0x8A;
    /// Transport layer from a device, long header, no application data.
    pub const TPL_LONG: u8 = 0x8B;
    /// Extended link layer: CC, access number.
    pub const ELL_SHORT: u8 = 0x8C;
    /// Extended link layer with session number and payload CRC (AES-CTR).
    pub const ELL_SHORT_SN: u8 = 0x8D;
    /// Extended link layer with receiver address.
    pub const ELL_LONG: u8 = 0x8E;
    /// Extended link layer with receiver address, session number and payload CRC.
    pub const ELL_LONG_SN: u8 = 0x8F;
    /// Authentication and fragmentation layer.
    pub const AFL: u8 = 0x90;
    /// Format frame (the record layout of compact frames), no header.
    pub const FORMAT_NONE: u8 = 0x69;
    /// Format frame, short header.
    pub const FORMAT_SHORT: u8 = 0x6A;
    /// Format frame, long header.
    pub const FORMAT_LONG: u8 = 0x6B;
    /// Compact frame, no header.
    pub const COMPACT_NONE: u8 = 0x79;
    /// Compact frame, short header.
    pub const COMPACT_SHORT: u8 = 0x7B;
    /// Compact frame, long header.
    pub const COMPACT_LONG: u8 = 0x73;
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
            0x20 => "breaker (electricity)",
            0x21 => "valve (gas or water)",
            0x25 => "customer unit (display)",
            0x28 => "waste water",
            0x29 => "garbage",
            0x31 => "communication controller",
            0x32 => "unidirectional repeater",
            0x33 => "bidirectional repeater",
            0x36 => "radio converter (system side)",
            0x37 => "radio converter (meter side)",
            0x38 => "wired adapter",
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

/// A meter's address: manufacturer, identification number, version and device type.
///
/// The same eight bytes appear in the wireless link layer and in a long header (where
/// they name the meter behind a repeater or a gateway). They select the key and form
/// part of the OMS initialisation vector.
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
    /// The identification number as printed on the meter, if it is valid BCD.
    pub fn id_number(&self) -> Option<u32> {
        crate::value::bcd_u32(self.id)
    }

    /// The address in link-layer order: manufacturer (LE), ID, version, medium.
    pub const fn to_bytes(self) -> [u8; 8] {
        let [m0, m1] = self.manufacturer.0.to_le_bytes();
        let [i0, i1, i2, i3] = self.id;
        [m0, m1, i0, i1, i2, i3, self.version, self.medium.0]
    }

    /// Parse link-layer order (manufacturer first).
    pub const fn from_link_bytes(b: [u8; 8]) -> Self {
        let [m0, m1, i0, i1, i2, i3, version, medium] = b;
        Self {
            manufacturer: Manufacturer(u16::from_le_bytes([m0, m1])),
            id: [i0, i1, i2, i3],
            version,
            medium: Medium(medium),
        }
    }
}

impl fmt::Display for Address {
    /// `KAM 12345678 water v27`; the raw ID in hex if it is not valid BCD.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.id_number() {
            write!(f, "{} {n:08} {} v{}", self.manufacturer, self.medium, self.version)
        } else {
            let [a, b, c, d] = self.id;
            write!(f, "{} {d:02X}{c:02X}{b:02X}{a:02X} {} v{}", self.manufacturer, self.medium, self.version)
        }
    }
}

/// The security mode from a transport header's configuration field (OMS Vol. 2
/// §7.2.4, Tables 19 and 20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecurityMode {
    /// The mode: 0 = none, 5 = AES-128-CBC with a static key, 7 = AES-128-CBC with a
    /// derived key and CMAC, others as defined in EN 13757-7.
    pub mode: u8,
    /// The number of encrypted 16-byte blocks; 0 = none, 15 = everything to the end.
    pub encrypted_blocks: u8,
}

impl SecurityMode {
    /// Read the mode from the 2-byte configuration field (as transmitted, LSB first).
    pub const fn from_config(config: [u8; 2]) -> Self {
        let [low, high] = config;
        Self { mode: high & 0x1F, encrypted_blocks: low >> 4 }
    }
}

/// The short header: access number, status and configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShortHeader {
    /// Access number, incremented by the meter per telegram.
    pub access_number: u8,
    /// Status byte (application errors, battery low, …).
    pub status: u8,
    /// Configuration field, as transmitted.
    pub config: [u8; 2],
    /// The configuration field extension: mode 7 one byte (KDF, key id), mode 10 two
    /// bytes (content index, tag size, KDF, key id), as a little-endian value.
    pub config_ext: Option<u16>,
    /// Mode 10: the message counter in the transport layer. Strictly increasing per meter.
    pub counter: Option<u32>,
}

impl ShortHeader {
    /// The security mode the configuration field announces.
    pub const fn security(&self) -> SecurityMode {
        SecurityMode::from_config(self.config)
    }
}

/// The long header: the meter's address plus the short header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongHeader {
    /// The meter (ID first on the wire here, unlike the link layer).
    pub address: Address,
    /// Access number, status and configuration.
    pub short: ShortHeader,
}

/// The transport-layer header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Header {
    /// No header.
    None,
    /// Short header.
    Short(ShortHeader),
    /// Long header.
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

    /// The short part (access number, status, configuration), if there is a header.
    pub const fn short(&self) -> Option<&ShortHeader> {
        match self {
            Self::None => None,
            Self::Short(s) | Self::Long(LongHeader { short: s, .. }) => Some(s),
        }
    }
}

/// The extended link layer (CI `8C`–`8F`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ell {
    /// Communication control (bidirectional, response delay, synchronous, hop, …).
    pub cc: u8,
    /// The link layer's access number (the TPL's is used for the mode-5 IV).
    pub access_number: u8,
    /// `8E`/`8F`: the receiver's address.
    pub receiver: Option<Address>,
    /// `8D`/`8F`: the session number (encryption in bits 31–29, time, session counter).
    pub session: Option<u32>,
}

impl Ell {
    /// Whether the session number announces AES-128-CTR encryption (ENC = 1).
    pub const fn encrypted(&self) -> bool {
        matches!(self.session, Some(sn) if sn >> 29 == 1)
    }
}

/// The authentication and fragmentation layer (CI `90`, OMS Vol. 2 §6, EN 13757-7).
///
/// Its MAC is verified only for security mode 7 (by [`Encrypted::decrypt`]); an AFL MAC in
/// front of a plaintext or mode-5 transport layer is parsed, not checked. A fragment
/// (fragment ID not 0) has [`Body::Fragment`]; join the fragments with
/// [`Reassembly`](crate::fragment::Reassembly).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Afl {
    /// Fragmentation control: bit 14 more fragments, 13–9 which fields are present,
    /// 7–0 fragment ID.
    pub fcl: u16,
    /// Message control: authentication type in bits 3–0.
    pub mcl: Option<u8>,
    /// Key information.
    pub key_info: Option<u16>,
    /// Message counter: strictly increasing per meter; compare it to detect replays.
    pub counter: Option<u32>,
    /// Message length: the bytes of the whole message from the TPL CI on (first fragment
    /// of a fragmented message only).
    pub length: Option<u16>,
    mac: [u8; 16],
    mac_len: u8,
}

impl Afl {
    /// More fragments follow this one.
    pub const fn more_fragments(&self) -> bool {
        self.fcl & 0x4000 != 0
    }

    /// The fragment ID: 0 for an unfragmented message, 1, 2, … for the fragments of one.
    pub const fn fragment_id(&self) -> u8 {
        self.fcl.to_le_bytes()[0]
    }

    /// The AFL of a reassembled message: message control, key information, counter and
    /// length from the first fragment, the MAC from the last (OMS Vol. 2 §6.2).
    pub(crate) const fn joined(first: &Self, last: &Self) -> Self {
        Self {
            fcl: (first.fcl & 0x3A00) | (last.fcl & 0x04FF),
            mcl: first.mcl,
            key_info: first.key_info,
            counter: first.counter,
            length: first.length,
            mac: last.mac,
            mac_len: last.mac_len,
        }
    }

    /// The message authentication code, as received (empty if absent).
    pub fn mac(&self) -> &[u8] {
        self.mac.get(..usize::from(self.mac_len)).unwrap_or(&[])
    }
}

/// What was cryptographically verified about a telegram. Only
/// [`Authenticity::Authenticated`] makes a radio reading evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authenticity {
    /// Nothing: plaintext, or still encrypted. Anyone in radio range can send this.
    None,
    /// Decrypted with the meter's key (mode 5, ELL, or mode 7 without a MAC):
    /// **confidential, but not integrity-protected.** These modes are malleable: without
    /// the key, an attacker can change bits of the plaintext through the IV inputs in the
    /// header (mode 5) or the CTR ciphertext with a fixed-up CRC (ELL), and `2F 2F` and the
    /// payload CRC still pass. And anyone holding the key, often shared by a fleet, can
    /// send anything. Not evidence of a reading.
    Encrypted,
    /// The AFL CMAC was verified with the key derived for this message (mode 7).
    Authenticated {
        /// The message counter the MAC covers. Reject a counter not greater than the
        /// last one accepted from this meter: the library keeps no state.
        counter: u32,
    },
}

/// How an encrypted layer is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Security {
    /// TPL mode 5: AES-128-CBC with a static key and an IV from address and access
    /// number.
    Mode5,
    /// TPL mode 7: AES-128-CBC with a per-message key derived from the AFL counter.
    Mode7,
    /// TPL mode 10: AES-128-CCM with a per-message key; the tag authenticates.
    Mode10,
    /// ELL: AES-128-CTR (CI `8D`/`8F`).
    EllCtr,
    /// Any other TPL security mode (not decrypted by this crate).
    Other(u8),
}

/// An encrypted layer and everything needed to open it.
#[derive(Clone, Copy, Debug)]
pub struct Encrypted<'a> {
    /// How it is protected.
    pub security: Security,
    /// The encrypted bytes, and any plain bytes after them.
    pub data: &'a [u8],
    /// How many leading bytes of `data` are encrypted.
    pub encrypted_len: usize,
    /// The meter whose key opens it, if known.
    pub meter: Option<Address>,
    #[cfg_attr(not(feature = "oms"), allow(dead_code))]
    pub(crate) ctx: Context,
}

/// What decryption needs from the layers around the ciphertext.
#[derive(Clone, Copy, Debug, Default)]
#[cfg_attr(not(feature = "oms"), allow(dead_code))]
pub(crate) struct Context {
    /// IV for mode 5 and the ELL; zero for mode 7.
    pub(crate) iv: [u8; 16],
    /// The TPL CI (part of the MAC input).
    pub(crate) ci: u8,
    /// The TPL header bytes after the CI (MAC input, CCM associated data): at most 12 +
    /// CFE (2) + key version (1).
    pub(crate) header: [u8; 16],
    pub(crate) header_len: u8,
    /// The layers parsed so far, copied into the decrypted result.
    pub(crate) ell: Option<Ell>,
    pub(crate) afl: Option<Afl>,
    pub(crate) tpl: Option<(u8, Header)>,
    pub(crate) link: Option<Address>,
    /// Parsing the plaintext of a decrypted ELL.
    pub(crate) inside_ell: bool,
    /// Mode 10: TPL message counter and tag.
    pub(crate) counter: Option<u32>,
    pub(crate) tag: [u8; 16],
    pub(crate) tag_len: u8,
}

/// The payload after the transport header.
#[derive(Clone, Debug)]
pub enum Body<'a> {
    /// Data records.
    Records(Records<'a>),
    /// An encrypted layer; open it with [`Encrypted::decrypt`] (feature `oms`).
    Encrypted(Encrypted<'a>),
    /// An application error (CI `6E`–`70`): the error code bytes (OMS Vol. 2 Table 40:
    /// `01` CI error, `03` too many records, `08` busy, …).
    ApplicationError(&'a [u8]),
    /// An alarm (CI `71`, `74`, `75`): the alarm status bytes.
    Alarm(&'a [u8]),
    /// A transport layer without application data (CI `80`, `8A`, `8B`): an ACK, an
    /// access demand or a request.
    Empty,
    /// A compact frame (CI `79`, `7B`, `73`): values only; expand it with the layout of
    /// an earlier full frame ([`Compact::expand`](crate::compact::Compact::expand)).
    Compact(crate::compact::Compact<'a>),
    /// A format frame (CI `69`, `6A`, `6B`): a record layout for compact frames.
    Format(crate::compact::Format<'a>),
    /// One fragment of a longer message (AFL fragment ID not 0): the bytes after the AFL,
    /// a piece of the transport layer. Join them with
    /// [`Reassembly`](crate::fragment::Reassembly).
    Fragment(&'a [u8]),
}

/// A parsed telegram above the link layer.
#[derive(Clone, Debug)]
pub struct Application<'a> {
    /// The extended link layer, if present.
    pub ell: Option<Ell>,
    /// The authentication and fragmentation layer, if present.
    pub afl: Option<Afl>,
    /// The transport layer's CI, or the ELL's while the ELL is still encrypted.
    pub ci: u8,
    /// The transport header.
    pub header: Header,
    /// What was cryptographically verified.
    pub authenticity: Authenticity,
    /// The payload.
    pub body: Body<'a>,
    meter: Option<Address>,
}

impl<'a> Application<'a> {
    /// Parse the bytes after a CI field: a wired telegram, or a wireless one when the
    /// link-layer address does not matter (it is needed for a mode-5 short header and
    /// the ELL: use [`Application::parse_with_link`]).
    pub fn parse(ci: u8, data: &'a [u8]) -> Result<Self> {
        Self::parse_with_link(ci, data, None)
    }

    /// Parse the bytes after a CI field of a wireless telegram from `link`.
    pub fn parse_with_link(ci: u8, data: &'a [u8], link: Option<Address>) -> Result<Self> {
        let ctx = Context { link, ..Context::default() };
        parse_layers(ci, data, ctx, Stage::Ell)
    }

    /// Parse a message reassembled from fragments: `data` from the TPL CI on, with the
    /// AFL joined from the first and last fragment.
    pub(crate) fn reassembled(
        data: &'a [u8],
        afl: Afl,
        ell: Option<Ell>,
        link: Option<Address>,
        decrypted: bool,
    ) -> Result<Self> {
        let (&ci, rest) = data.split_first().ok_or(Error::UnexpectedEnd)?;
        let ctx = Context { link, ell, afl: Some(afl), inside_ell: decrypted, ..Context::default() };
        let mut app = parse_layers(ci, rest, ctx, Stage::Tpl)?;
        if decrypted && !matches!(app.body, Body::Encrypted(_)) && app.authenticity == Authenticity::None {
            app.authenticity = Authenticity::Encrypted;
        }
        Ok(app)
    }

    /// The meter: the long header's address if there is one (a meter behind a radio
    /// adapter or gateway), the link layer's otherwise.
    pub const fn meter(&self) -> Option<Address> {
        self.meter
    }

    /// A compact frame expanded with a layout from `formats` into `out`, as an
    /// application with the same meter, header and authenticity and the full records
    /// ([`Compact::expand`](crate::compact::Compact::expand)); any other body is
    /// [`Error::UnsupportedCi`].
    pub fn expand<'o, F: crate::compact::Formats + ?Sized>(
        &self,
        formats: &F,
        out: &'o mut [u8],
    ) -> Result<Application<'o>> {
        let Body::Compact(c) = &self.body else { return Err(Error::UnsupportedCi(self.ci)) };
        let records = c.expand(formats, out)?;
        Ok(Application {
            ell: self.ell,
            afl: self.afl,
            ci: self.ci,
            header: self.header,
            authenticity: self.authenticity,
            body: Body::Records(records),
            meter: self.meter,
        })
    }

    /// The records, if the body is plain records.
    pub fn records(&self) -> Option<Records<'a>> {
        match &self.body {
            Body::Records(r) => Some(r.clone()),
            _ => None,
        }
    }
}

#[cfg(feature = "oms")]
impl Encrypted<'_> {
    /// Decrypt (and, for mode 7 with an AFL MAC, authenticate) this layer into `out`,
    /// and parse what is inside.
    ///
    /// | Security | Key | Verified by | Authenticity |
    /// |---|---|---|---|
    /// | mode 5 | the meter's | `2F 2F` | [`Authenticity::Encrypted`] |
    /// | mode 7 | derived from the meter's and the AFL counter | AFL CMAC, then `2F 2F` | [`Authenticity::Authenticated`] (without a MAC: `Encrypted`) |
    /// | ELL | the meter's | payload CRC | `Encrypted`; the inner layers may add more |
    ///
    /// `out` needs `self.data.len()` bytes. On any failed check the result is an error
    /// naming the check ([`Check`]), never partial records and never key material. An
    /// ELL may contain a TPL that is itself encrypted: the result's body is then
    /// [`Body::Encrypted`] again.
    pub fn decrypt<'o, K: crate::oms::Keys + ?Sized>(
        &self,
        keys: &K,
        out: &'o mut [u8],
    ) -> Result<Application<'o>> {
        use crate::oms;
        let meter = self.meter.ok_or(Error::NoKey)?;
        let key = keys.key(&meter).ok_or(Error::NoKey)?;
        let buf = out.get_mut(..self.data.len()).ok_or(Error::OutputTooSmall)?;
        buf.copy_from_slice(self.data);
        let ctx = &self.ctx;
        let authenticity = match self.security {
            Security::EllCtr => {
                oms::ctr_apply(key, &ctx.iv, buf);
                let plain: &'o [u8] = buf;
                let (crc, rest) = plain.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
                if crc16(rest) != u16::from_le_bytes(*crc) {
                    return Err(Error::Verification(Check::PayloadCrc));
                }
                let (&next, rest) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
                let inner = Context { link: ctx.link, ell: ctx.ell, inside_ell: true, ..Context::default() };
                let mut app = parse_layers(next, rest, inner, Stage::Afl)?;
                if !matches!(app.body, Body::Encrypted(_)) && app.authenticity == Authenticity::None {
                    app.authenticity = Authenticity::Encrypted;
                }
                return Ok(app);
            }
            Security::Mode5 => {
                let enc = buf.get_mut(..self.encrypted_len).ok_or(Error::UnexpectedEnd)?;
                oms::cbc_decrypt(key, &ctx.iv, enc)?;
                Authenticity::Encrypted
            }
            Security::Mode7 => {
                let afl = ctx.afl.ok_or(Error::Verification(Check::MissingCounter))?;
                let counter = afl.counter.ok_or(Error::Verification(Check::MissingCounter))?;
                let c = counter.to_le_bytes();
                let mut authenticity = Authenticity::Encrypted;
                // The message control announces a MAC (AT 5–7) that is not there: stripped.
                if afl.mac().is_empty() && afl.mcl.and_then(mac_len).is_some() {
                    return Err(Error::Verification(Check::Mac));
                }
                if !afl.mac().is_empty() {
                    // OMS Vol. 2 §9.3.3: CMAC(Kmac, MCL ‖ MCR ‖ [ML] ‖ TPL CI … end), verified
                    // over the ciphertext before decrypting.
                    let kmac = oms::derive(key, 0x01, c, meter.id);
                    let ml = afl.length.map(u16::to_le_bytes);
                    let header = ctx.header.get(..usize::from(ctx.header_len)).unwrap_or(&[]);
                    let tag = oms::cmac(
                        &kmac,
                        &[
                            &[afl.mcl.unwrap_or(0)],
                            &c,
                            ml.as_ref().map_or(&[][..], |m| &m[..]),
                            &[ctx.ci],
                            header,
                            self.data,
                        ],
                    );
                    let mac = afl.mac();
                    if !oms::mac_matches(tag.get(..mac.len()).unwrap_or(&[]), mac) {
                        return Err(Error::Verification(Check::Mac));
                    }
                    authenticity = Authenticity::Authenticated { counter };
                }
                let kenc = oms::derive(key, 0x00, c, meter.id);
                let enc = buf.get_mut(..self.encrypted_len).ok_or(Error::UnexpectedEnd)?;
                oms::cbc_decrypt(&kenc, &[0; 16], enc)?;
                authenticity
            }
            Security::Mode10 => return self.open_ccm(key, meter, buf),
            Security::Other(mode) => return Err(Error::UnsupportedSecurity(mode)),
        };
        let plain: &'o [u8] = buf;
        if matches!(ctx.ci, ci::COMPACT_SHORT | ci::COMPACT_LONG) {
            // No public vector settles whether 2F 2F is inside the full-frame CRC; refuse
            // rather than guess. (ELL-encrypted compact frames, the common case, work.)
            return Err(Error::UnsupportedCi(ctx.ci));
        }
        if !plain.starts_with(&[0x2F, 0x2F]) {
            return Err(Error::Verification(Check::Marker));
        }
        let (ci, header) = ctx.tpl.unwrap_or((ctx.ci, Header::None));
        Ok(Application {
            ell: ctx.ell,
            afl: ctx.afl,
            ci,
            header,
            authenticity,
            body: Body::Records(Records::decrypted(plain, self.encrypted_len)),
            meter: self.meter,
        })
    }

    /// Mode 10: verify the CCM tag and decrypt (OMS Vol. 2 §9.3.7).
    fn open_ccm<'o>(
        &self,
        key: &crate::oms::Key,
        meter: Address,
        buf: &'o mut [u8],
    ) -> Result<Application<'o>> {
        use crate::oms;
        let ctx = &self.ctx;
        // OMS Vol. 2 §9.3.7: Kenc from the TPL counter; nonce M ‖ A ‖ 00 ‖ counter
        // (big-endian); associated data CI ‖ TPL header ‖ CFE ‖ [key version] ‖ the
        // unencrypted tail, if any.
        let counter = ctx.counter.ok_or(Error::Verification(Check::MissingCounter))?;
        let kenc = oms::derive(key, 0x00, counter.to_le_bytes(), meter.id);
        let [m0, m1, i0, i1, i2, i3, v, t] = meter.to_bytes();
        let [c0, c1, c2, c3] = counter.to_be_bytes();
        let nonce = [m0, m1, i0, i1, i2, i3, v, t, 0, c0, c1, c2, c3];
        let header = ctx.header.get(..usize::from(ctx.header_len)).unwrap_or(&[]);
        // Bytes after the encrypted part are authenticated as associated data, or the
        // telegram is refused: a tail outside the tag would let anyone append records to
        // an "authenticated" message (EN 13757-7 itself is not public).
        let tail = self.data.get(self.encrypted_len..).unwrap_or(&[]);
        let mut aad = [0u8; 1 + 16 + 256];
        let aad_len = 1 + header.len() + tail.len();
        if let Some((first, rest)) = aad.split_first_mut() {
            *first = ctx.ci;
            rest.get_mut(..header.len()).ok_or(Error::InvalidLength)?.copy_from_slice(header);
            rest.get_mut(header.len()..header.len() + tail.len())
                .ok_or(Error::InvalidLength)?
                .copy_from_slice(tail);
        }
        let tag = ctx.tag.get(..usize::from(ctx.tag_len)).unwrap_or(&[]);
        let enc = buf.get_mut(..self.encrypted_len).ok_or(Error::UnexpectedEnd)?;
        oms::ccm_decrypt(&kenc, &nonce, aad.get(..aad_len).unwrap_or(&[]), enc, tag)?;
        let plain: &'o [u8] = buf;
        let (ci, header) = ctx.tpl.unwrap_or((ctx.ci, Header::None));
        Ok(Application {
            ell: ctx.ell,
            afl: ctx.afl,
            ci,
            header,
            authenticity: Authenticity::Authenticated { counter },
            body: Body::Records(Records::decrypted(plain, self.encrypted_len)),
            meter: self.meter,
        })
    }
}

/// Which layers may still come: each at most once, in this order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Ell,
    Afl,
    Tpl,
}

fn address_id_first(b: [u8; 8]) -> Address {
    let [i0, i1, i2, i3, m0, m1, version, medium] = b;
    Address::from_link_bytes([m0, m1, i0, i1, i2, i3, version, medium])
}

#[allow(clippy::too_many_lines)]
fn parse_layers(ci: u8, data: &[u8], mut ctx: Context, stage: Stage) -> Result<Application<'_>> {
    match ci {
        ci::ELL_SHORT | ci::ELL_SHORT_SN | ci::ELL_LONG | ci::ELL_LONG_SN => {
            if stage > Stage::Ell {
                return Err(Error::UnexpectedLayer(ci));
            }
            let (&[cc, access], mut rest) = data.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
            let mut ell = Ell { cc, access_number: access, receiver: None, session: None };
            if matches!(ci, ci::ELL_LONG | ci::ELL_LONG_SN) {
                let (a, r) = rest.split_first_chunk::<8>().ok_or(Error::UnexpectedEnd)?;
                ell.receiver = Some(Address::from_link_bytes(*a));
                rest = r;
            }
            if matches!(ci, ci::ELL_SHORT_SN | ci::ELL_LONG_SN) {
                let (sn, r) = rest.split_first_chunk::<4>().ok_or(Error::UnexpectedEnd)?;
                let session = u32::from_le_bytes(*sn);
                ell.session = Some(session);
                ctx.ell = Some(ell);
                if ell.encrypted() {
                    // IV: M ‖ A of the transmitter ‖ CC without H and R ‖ SN ‖ FN 0 ‖ BC 0.
                    let mut iv = [0u8; 16];
                    if let Some(link) = ctx.link {
                        let [a, b, c, d, e, f, g, h] = link.to_bytes();
                        let [s0, s1, s2, s3] = *sn;
                        iv = [a, b, c, d, e, f, g, h, cc & !0x12, s0, s1, s2, s3, 0, 0, 0];
                    }
                    ctx.iv = iv;
                    return Ok(Application {
                        ell: Some(ell),
                        afl: None,
                        ci,
                        header: Header::None,
                        authenticity: Authenticity::None,
                        meter: ctx.link,
                        body: Body::Encrypted(Encrypted {
                            security: Security::EllCtr,
                            data: r,
                            encrypted_len: r.len(),
                            meter: ctx.link,
                            ctx,
                        }),
                    });
                }
                // Not encrypted: the payload CRC is plain and covers the rest.
                let (crc, r) = r.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
                if crc16(r) != u16::from_le_bytes(*crc) {
                    return Err(Error::Verification(Check::PayloadCrc));
                }
                rest = r;
            }
            ctx.ell = Some(ell);
            let (&next, rest) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
            parse_layers(next, rest, ctx, Stage::Afl)
        }
        ci::AFL => {
            if stage > Stage::Afl {
                return Err(Error::UnexpectedLayer(ci));
            }
            let (&len, rest) = data.split_first().ok_or(Error::UnexpectedEnd)?;
            let afl_bytes = rest.get(..usize::from(len)).ok_or(Error::UnexpectedEnd)?;
            let rest = rest.get(usize::from(len)..).ok_or(Error::UnexpectedEnd)?;
            let afl = parse_afl(afl_bytes)?;
            ctx.afl = Some(afl);
            if afl.fragment_id() != 0 {
                // OMS Vol. 2 §7.3: only the first fragment carries the TPL, cut anywhere.
                return Ok(Application {
                    ell: ctx.ell,
                    afl: Some(afl),
                    ci,
                    header: Header::None,
                    authenticity: Authenticity::None,
                    body: Body::Fragment(rest),
                    meter: ctx.link,
                });
            }
            let (&next, rest) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
            parse_layers(next, rest, ctx, Stage::Tpl)
        }
        _ => parse_tpl(ci, data, ctx),
    }
}

/// AT 5, 6, 7: AES-CMAC truncated to 8, 12, 16 bytes (OMS Vol. 2 §6.2.1).
pub(crate) const fn mac_len(mcl: u8) -> Option<usize> {
    match mcl & 0x0F {
        5 => Some(8),
        6 => Some(12),
        7 => Some(16),
        _ => None,
    }
}

fn parse_afl(b: &[u8]) -> Result<Afl> {
    let (fcl, mut rest) = b.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
    let fcl = u16::from_le_bytes(*fcl);
    let mut afl =
        Afl { fcl, mcl: None, key_info: None, counter: None, length: None, mac: [0; 16], mac_len: 0 };
    if fcl & 0x2000 != 0 {
        let (&mcl, r) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
        afl.mcl = Some(mcl);
        rest = r;
    }
    if fcl & 0x0200 != 0 {
        let (ki, r) = rest.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
        afl.key_info = Some(u16::from_le_bytes(*ki));
        rest = r;
    }
    if fcl & 0x0800 != 0 {
        let (c, r) = rest.split_first_chunk::<4>().ok_or(Error::UnexpectedEnd)?;
        afl.counter = Some(u32::from_le_bytes(*c));
        rest = r;
    }
    if fcl & 0x0400 != 0 {
        // With MCL: its authentication type sets the length. Without (the last fragment of
        // a message, OMS Vol. 2 §6.2.4): the MAC fills the AFL up to the ML field.
        let n = match afl.mcl {
            Some(mcl) => mac_len(mcl).ok_or(Error::InvalidLength)?,
            None => rest.len().saturating_sub(if fcl & 0x1000 != 0 { 2 } else { 0 }),
        };
        if !matches!(n, 8 | 12 | 16) {
            return Err(Error::InvalidLength);
        }
        let mac = rest.get(..n).ok_or(Error::UnexpectedEnd)?;
        afl.mac.get_mut(..n).ok_or(Error::InvalidLength)?.copy_from_slice(mac);
        afl.mac_len = u8::try_from(n).map_err(|_| Error::InvalidLength)?;
        rest = rest.get(n..).unwrap_or(&[]);
    }
    if fcl & 0x1000 != 0 {
        let (l, _) = rest.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
        afl.length = Some(u16::from_le_bytes(*l));
    }
    Ok(afl)
}

#[derive(Clone, Copy)]
enum Kind {
    Records,
    Error,
    Alarm,
    Empty,
    Compact,
    Format,
}

#[allow(clippy::too_many_lines)] // one table-driven pass over the transport header
fn parse_tpl(ci: u8, data: &[u8], mut ctx: Context) -> Result<Application<'_>> {
    // (header length, what follows)
    let (len, kind) = match ci {
        ci::RSP_LONG | ci::CMD_LONG => (12, Kind::Records),
        ci::RSP_SHORT | ci::CMD_SHORT => (4, Kind::Records),
        ci::RSP_NONE | ci::SND_UD => (0, Kind::Records),
        ci::ERROR_LONG => (12, Kind::Error),
        ci::ERROR_SHORT => (4, Kind::Error),
        ci::ERROR_NONE => (0, Kind::Error),
        ci::ALARM_LONG => (12, Kind::Alarm),
        ci::ALARM_SHORT => (4, Kind::Alarm),
        ci::ALARM_NONE => (0, Kind::Alarm),
        ci::TPL_TO_DEVICE | ci::TPL_LONG => (12, Kind::Empty),
        ci::TPL_SHORT => (4, Kind::Empty),
        ci::COMPACT_NONE => (0, Kind::Compact),
        ci::COMPACT_SHORT => (4, Kind::Compact),
        ci::COMPACT_LONG => (12, Kind::Compact),
        ci::FORMAT_NONE => (0, Kind::Format),
        ci::FORMAT_SHORT => (4, Kind::Format),
        ci::FORMAT_LONG => (12, Kind::Format),
        ci::ELL_SHORT | ci::ELL_SHORT_SN | ci::ELL_LONG | ci::ELL_LONG_SN | ci::AFL => {
            return Err(Error::UnexpectedLayer(ci));
        }
        other => return Err(Error::UnsupportedCi(other)),
    };
    let head = data.get(..len).ok_or(Error::UnexpectedEnd)?;
    let mut rest = data.get(len..).ok_or(Error::UnexpectedEnd)?;
    let header = match *head {
        [i0, i1, i2, i3, m0, m1, version, medium, access, status, c0, c1] => Header::Long(LongHeader {
            address: address_id_first([i0, i1, i2, i3, m0, m1, version, medium]),
            short: ShortHeader {
                access_number: access,
                status,
                config: [c0, c1],
                config_ext: None,
                counter: None,
            },
        }),
        [access, status, c0, c1] => Header::Short(ShortHeader {
            access_number: access,
            status,
            config: [c0, c1],
            config_ext: None,
            counter: None,
        }),
        _ => Header::None,
    };
    let security = header.security();
    let mut header = header;
    let mut header_len = len;
    // Bytes after the fixed header that belong to it: the CFE, a key version.
    let mut ext = [0u8; 3];
    let mut ext_len = 0usize;
    if security.mode == 7 {
        // Mode 7 has a configuration field extension byte (OMS Vol. 2 Table 21).
        let (&cfe, r) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
        rest = r;
        ext[0] = cfe;
        ext_len = 1;
        if let Header::Short(s) | Header::Long(LongHeader { short: s, .. }) = &mut header {
            s.config_ext = Some(u16::from(cfe));
        }
    }
    if security.mode == 10 {
        // Mode 10: CFE (2 bytes), key version if CFE bit 6, message counter (4) (OMS Vol. 2
        // §7.2.4.5); the tag is the end of the message.
        let (cfe, r) = rest.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
        let [e0, e1] = *cfe;
        rest = r;
        ext = [e0, e1, 0];
        ext_len = 2;
        let cfe = u16::from_le_bytes([e0, e1]);
        if cfe & 0x0040 != 0 {
            let (&kv, r) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
            rest = r;
            ext[2] = kv;
            ext_len = 3;
        }
        let (mc, r) = rest.split_first_chunk::<4>().ok_or(Error::UnexpectedEnd)?;
        rest = r;
        let counter = u32::from_le_bytes(*mc);
        ctx.counter = Some(counter);
        let tag_len = 4 + 4 * usize::from((cfe >> 8) & 0x03);
        let split = rest.len().checked_sub(tag_len).ok_or(Error::UnexpectedEnd)?;
        let (data, tag) = rest.split_at(split);
        ctx.tag.get_mut(..tag_len).ok_or(Error::InvalidLength)?.copy_from_slice(tag);
        ctx.tag_len = u8::try_from(tag_len).map_err(|_| Error::InvalidLength)?;
        rest = data;
        if let Header::Short(s) | Header::Long(LongHeader { short: s, .. }) = &mut header {
            s.config_ext = Some(cfe);
            s.counter = Some(counter);
        }
    }
    header_len += ext_len;
    let meter = match header {
        Header::Long(l) => Some(l.address),
        _ => ctx.link,
    };
    let app = |body| Application {
        ell: ctx.ell,
        afl: ctx.afl,
        ci,
        header,
        authenticity: Authenticity::None,
        body,
        meter,
    };
    // Mode 10 is always opened: with nothing encrypted the tag still authenticates.
    // Only modes 5 and 7 count 16-byte blocks in NNNN (0 = plaintext). Mode 10 is always
    // opened (with nothing encrypted the tag still authenticates); any other mode (13: TLS)
    // has its own layout and stays encrypted.
    if security.mode == 0 || (security.encrypted_blocks == 0 && matches!(security.mode, 5 | 7)) {
        let body = match kind {
            // Behind a decrypted ELL every byte came out of the ciphertext.
            Kind::Records if ctx.inside_ell => Body::Records(Records::decrypted(rest, rest.len())),
            Kind::Records => Body::Records(Records::new(rest)),
            Kind::Error => Body::ApplicationError(rest),
            Kind::Alarm => Body::Alarm(rest),
            Kind::Empty => Body::Empty,
            Kind::Compact => Body::Compact(crate::compact::Compact::parse(rest, ctx.inside_ell)?),
            Kind::Format => {
                let (&len, rest) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
                let body = rest.get(..usize::from(len)).ok_or(Error::UnexpectedEnd)?;
                let (sig, headers) = body.split_first_chunk::<2>().ok_or(Error::UnexpectedEnd)?;
                let format = crate::compact::Format::received(u16::from_le_bytes(*sig), headers)
                    .ok_or(Error::Verification(Check::FullFrameCrc))?;
                Body::Format(format)
            }
        };
        return Ok(app(body));
    }
    let encrypted_len = if security.mode == 10 {
        // Mode 10 counts encrypted bytes in the whole low byte; FF means everything.
        match header.short().map_or(0, |s| s.config[0]) {
            0xFF => rest.len(),
            n => usize::from(n),
        }
    } else if security.encrypted_blocks == 0x0F || !matches!(security.mode, 5 | 7) {
        // 15 blocks means "to the end"; other modes: the whole rest is opaque.
        rest.len()
    } else {
        usize::from(security.encrypted_blocks) * 16
    };
    if encrypted_len > rest.len() || (matches!(security.mode, 5 | 7) && encrypted_len % 16 != 0) {
        return Err(Error::Verification(Check::Length));
    }
    let security = match security.mode {
        5 => Security::Mode5,
        7 => Security::Mode7,
        10 => Security::Mode10,
        other => Security::Other(other),
    };
    if let (Security::Mode5, Some(m), Some(s)) = (security, meter, header.short()) {
        let [a, b, c, d, e, f, g, h] = m.to_bytes();
        let x = s.access_number;
        ctx.iv = [a, b, c, d, e, f, g, h, x, x, x, x, x, x, x, x];
    }
    ctx.ci = ci;
    ctx.header.get_mut(..len).ok_or(Error::InvalidLength)?.copy_from_slice(head);
    ctx.header
        .get_mut(len..len + ext_len)
        .ok_or(Error::InvalidLength)?
        .copy_from_slice(ext.get(..ext_len).unwrap_or(&[]));
    ctx.header_len = u8::try_from(header_len).map_err(|_| Error::InvalidLength)?;
    ctx.tpl = Some((ci, header));
    Ok(app(Body::Encrypted(Encrypted { security, data: rest, encrypted_len, meter, ctx })))
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
        // OMS mode 5 with 1 encrypted block: configuration 0x0510, sent as 10 05.
        let s = SecurityMode::from_config([0x10, 0x05]);
        assert_eq!(s, SecurityMode { mode: 5, encrypted_blocks: 1 });
        let mut data = [0u8; 4 + 18];
        data[..4].copy_from_slice(&[0x01, 0x00, 0x10, 0x05]);
        let app = Application::parse(ci::RSP_SHORT, &data).unwrap();
        let Body::Encrypted(e) = app.body else { panic!() };
        assert_eq!((e.security, e.encrypted_len, e.data.len()), (Security::Mode5, 16, 18));
        // More blocks announced than present.
        assert_eq!(
            Application::parse(ci::RSP_SHORT, &data[..10]).unwrap_err(),
            Error::Verification(Check::Length)
        );
    }

    // 002-oms-security/SR-004: each layer once, in order.
    #[test]
    fn layer_order_is_enforced() {
        assert_eq!(Application::parse(0x7A, &[0, 0, 0, 0]).unwrap().authenticity, Authenticity::None);
        // AFL after the TPL is not a TPL.
        assert_eq!(
            Application::parse(0x90, &[0x02, 0x00, 0x00, 0x8C, 0, 0, 0x78]).unwrap_err(),
            Error::UnexpectedLayer(0x8C)
        );
        assert_eq!(
            Application::parse(0x8C, &[0x20, 0x01, 0x8C, 0, 0, 0x78]).unwrap_err(),
            Error::UnexpectedLayer(0x8C)
        );
        assert_eq!(Application::parse(0x42, &[]).unwrap_err(), Error::UnsupportedCi(0x42));
    }
}
