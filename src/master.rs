//! A wired M-Bus master, sans-I/O (EN 13757-2/-3): the frames a master sends, a readout
//! that handles the frame count bit and multi-telegram replies, and a secondary-address
//! search.
//!
//! Nothing here opens a port or reads a clock. Each state machine says what to send and
//! how long to wait for a reply ([`Request`]); you write the bytes, wait, and hand back
//! what arrived ([`Readout::reply`], [`Search::reply`]) or that nothing did
//! ([`Readout::timeout`], [`Search::timeout`]).
//!
//! ```
//! use mbus_kit::master::{Event, Readout, Target};
//!
//! let mut readout = Readout::new(Target::Primary(5));
//! # let replies: [&[u8]; 2] = [&[0xE5], &[0x68, 0x1F, 0x1F, 0x68, 0x08, 0x05, 0x72, 0x78, 0x56, 0x34, 0x12, 0x24, 0x40, 0x01, 0x07, 0x55, 0x00, 0x00, 0x00, 0x03, 0x13, 0x15, 0x31, 0x00, 0xDA, 0x02, 0x3B, 0x13, 0x01, 0x8B, 0x60, 0x04, 0x37, 0x18, 0x02, 0x1B, 0x16]];
//! # let mut replies = replies.into_iter();
//! while let Some(request) = readout.next_request() {
//!     // port.write(request.bytes()); wait up to request.timeout_ms(Baud::B2400) for a frame …
//!     let reply: &[u8] = replies.next().unwrap();
//!     match readout.reply(reply) {
//!         Event::Telegram(app) => {
//!             for r in app.records().into_iter().flatten() {
//!                 println!("{}", r.unwrap());
//!             }
//!         }
//!         Event::NeedMore => { /* read more bytes and call reply() again */ }
//!         _ => {}
//!     }
//! }
//! assert!(readout.is_complete()); // or readout.outcome(): why it is not
//! ```
//!
//! Hand `reply` exactly one frame ([`wired::scan`] finds it behind noise or an echo).
//! A readout delivers only telegrams from the meter it addresses and says how it ended
//! ([`Outcome`]): a telegram limit or a broken page is never "complete".

use core::fmt;

use crate::app::{Address, Application, Manufacturer, Medium};
use crate::time::DateTime;
use crate::wired::{self, BROADCAST, Frame, NETWORK_LAYER, control};
use crate::{Error, Result};

/// CI values a master sends (EN 13757-3, m-bus.com 6.1).
pub mod ci {
    /// Application reset.
    pub const APPLICATION_RESET: u8 = 0x50;
    /// Data send (records to the slave).
    pub const DATA_SEND: u8 = 0x51;
    /// Selection of a slave by secondary address.
    pub const SELECT: u8 = 0x52;
    /// Baud rate 300 … 38400: `B8` + index.
    pub const BAUD_300: u8 = 0xB8;
}

/// A secondary address to select, each part optional (a wildcard).
///
/// The identification number is eight BCD digits; any digit may be a wildcard (`F` on
/// the wire). Manufacturer, version and medium are wildcarded as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SecondaryAddress {
    /// The eight digits, most significant first; `None` is a wildcard.
    pub digits: [Option<u8>; 8],
    /// Manufacturer, or any.
    pub manufacturer: Option<Manufacturer>,
    /// Version, or any.
    pub version: Option<u8>,
    /// Medium, or any.
    pub medium: Option<Medium>,
}

impl SecondaryAddress {
    /// Every meter: all wildcards.
    pub const ANY: Self = Self { digits: [None; 8], manufacturer: None, version: None, medium: None };

    /// The meter with this identification number (up to 8 decimal digits), any
    /// manufacturer, version and medium. `None` above 99 999 999.
    pub fn id(number: u32) -> Option<Self> {
        if number > 99_999_999 {
            return None;
        }
        // const fn: unrolled, no indexing.
        #[allow(clippy::cast_possible_truncation)] // each digit < 10
        let d = |k: u32| Some((number / k % 10) as u8);
        let digits = [d(10_000_000), d(1_000_000), d(100_000), d(10_000), d(1_000), d(100), d(10), d(1)];
        Some(Self { digits, ..Self::ANY })
    }

    /// Exactly this meter: number, manufacturer, version and medium.
    pub fn exact(address: &Address) -> Self {
        let mut digits = [None; 8];
        let [a, b, c, d] = address.id;
        for (slot, byte) in digits.chunks_exact_mut(2).zip([d, c, b, a]) {
            if let [high, low] = slot {
                *high = Some(byte >> 4).filter(|&x| x <= 9);
                *low = Some(byte & 0x0F).filter(|&x| x <= 9);
            }
        }
        Self {
            digits,
            manufacturer: Some(address.manufacturer),
            version: Some(address.version),
            medium: Some(address.medium),
        }
    }

    /// Parse a pattern of 8 characters, digits or `F`/`*`/`?` wildcards, e.g.
    /// `"1234FFFF"`. Shorter patterns are padded with wildcards on the right.
    pub fn pattern(text: &str) -> Option<Self> {
        let mut out = Self::ANY;
        for (n, c) in text.bytes().enumerate() {
            let slot = out.digits.get_mut(n)?;
            *slot = match c {
                b'0'..=b'9' => Some(c - b'0'),
                b'F' | b'f' | b'*' | b'?' => None,
                _ => return None,
            };
        }
        Some(out)
    }

    /// The identification number as transmitted (BCD, LSB first, `F` for wildcards).
    pub fn id_bytes(&self) -> [u8; 4] {
        let nibble = |d: Option<u8>| d.unwrap_or(0x0F);
        let mut out = [0u8; 4];
        for (i, pair) in self.digits.chunks_exact(2).enumerate() {
            if let ([high, low], Some(slot)) = (pair, out.get_mut(3 - i)) {
                *slot = (nibble(*high) << 4) | nibble(*low);
            }
        }
        out
    }

    /// Whether `address` matches.
    pub fn matches(&self, address: &Address) -> bool {
        let exact = Self::exact(address);
        self.digits.iter().zip(exact.digits).all(|(want, got)| want.is_none() || *want == got)
            && self.manufacturer.is_none_or(|m| m == address.manufacturer)
            && self.version.is_none_or(|v| v == address.version)
            && self.medium.is_none_or(|m| m == address.medium)
    }

    /// The 8 data bytes of a selection: ID (BCD, LSB first), manufacturer (LE), version,
    /// medium; wildcards as `F` nibbles, `FFFF` and `FF`.
    pub fn to_bytes(&self) -> [u8; 8] {
        let [i0, i1, i2, i3] = self.id_bytes();
        let [m0, m1] = self.manufacturer.map_or(0xFFFF, |m| m.0).to_le_bytes();
        [i0, i1, i2, i3, m0, m1, self.version.unwrap_or(0xFF), self.medium.map_or(0xFF, |m| m.0)]
    }
}

impl fmt::Display for SecondaryAddress {
    /// `12345678`, `1234FFFF KAM v27 water`, wildcards as `F`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for d in self.digits {
            match d {
                Some(d) => write!(f, "{d}")?,
                None => f.write_str("F")?,
            }
        }
        if let Some(m) = self.manufacturer {
            write!(f, " {m}")?;
        }
        if let Some(v) = self.version {
            write!(f, " v{v}")?;
        }
        if let Some(m) = self.medium {
            write!(f, " {m}")?;
        }
        Ok(())
    }
}

/// Baud rates of wired M-Bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Baud {
    /// 300 Bd.
    B300,
    /// 600 Bd.
    B600,
    /// 1200 Bd.
    B1200,
    /// 2400 Bd (the default of most meters).
    B2400,
    /// 4800 Bd.
    B4800,
    /// 9600 Bd.
    B9600,
    /// 19200 Bd.
    B19200,
    /// 38400 Bd.
    B38400,
}

impl Baud {
    /// Bits per second.
    pub const fn bits_per_second(self) -> u32 {
        match self {
            Self::B300 => 300,
            Self::B600 => 600,
            Self::B1200 => 1200,
            Self::B2400 => 2400,
            Self::B4800 => 4800,
            Self::B9600 => 9600,
            Self::B19200 => 19200,
            Self::B38400 => 38400,
        }
    }

    const fn ci(self) -> u8 {
        ci::BAUD_300
            + match self {
                Self::B300 => 0,
                Self::B600 => 1,
                Self::B1200 => 2,
                Self::B2400 => 3,
                Self::B4800 => 4,
                Self::B9600 => 5,
                Self::B19200 => 6,
                Self::B38400 => 7,
            }
    }
}

/// A frame to send and how long to wait for its reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    bytes: [u8; 32],
    len: u8,
    reply: bool,
}

/// The response timeout at `baud`: 330 bit times plus 50 ms (EN 13757-2 via m-bus.com
/// 5.4), in milliseconds rounded up: 188 ms at 2400 Bd, 85 ms at 9600 Bd.
pub const fn response_timeout_ms(baud: Baud) -> u32 {
    (330 * 1000u32).div_ceil(baud.bits_per_second()) + 50
}

/// The default number of repetitions of a request that got no valid reply.
pub const RETRIES: u8 = 2;

impl Request {
    const EMPTY: Self = Self { bytes: [0; 32], len: 0, reply: false };

    fn from_frame(frame: &Frame<'_>, reply: bool) -> Result<Self> {
        let mut bytes = [0u8; 32];
        let n = frame.encode(&mut bytes)?;
        Ok(Self { bytes, len: u8::try_from(n).map_err(|_| Error::OutputTooSmall)?, reply })
    }

    /// The bytes to write to the bus.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }

    /// How long to wait for the start of the reply at `baud`, in milliseconds
    /// ([`response_timeout_ms`]); 0 when no reply is expected (broadcast).
    pub const fn timeout_ms(&self, baud: Baud) -> u32 {
        if self.reply { response_timeout_ms(baud) } else { 0 }
    }

    /// The same frame with the frame count bit set or cleared (checksum recomputed).
    /// EN 13757-2: after `SND_NKE`, and to address `FD` after every selection, the next
    /// `SND_UD` carries FCB = 1; it toggles after each acknowledged one.
    #[must_use]
    pub fn with_fcb(mut self, fcb: bool) -> Self {
        let len = usize::from(self.len);
        let (c_at, sum) = match self.bytes() {
            [0x10, ..] => (1, 1..3),
            [0x68, ..] if len >= 6 => (4, 4..len - 2),
            _ => return self,
        };
        if let Some(c) = self.bytes.get_mut(c_at) {
            *c = (*c & !control::FCB) | if fcb { control::FCB } else { 0 };
        }
        let cs = self.bytes.get(sum).map_or(0, |b| b.iter().fold(0u8, |s, &x| s.wrapping_add(x)));
        if let Some(slot) = self.bytes.get_mut(len - 2) {
            *slot = cs;
        }
        self
    }

    /// Whether a reply is expected at all.
    pub const fn expects_reply(&self) -> bool {
        self.reply
    }
}

const fn timeout_for(address: u8) -> bool {
    address != BROADCAST
}

/// `SND_NKE`: reset the link layer of a slave (and its FCB); to [`NETWORK_LAYER`], deselect.
pub fn link_reset(address: u8) -> Request {
    // A short frame always fits the 32-byte request.
    Request::from_frame(&wired::snd_nke(address), timeout_for(address)).unwrap_or(Request::EMPTY)
}

fn snd_ud(address: u8, fcb: bool, ci: u8, data: &[u8]) -> Result<Request> {
    let c = control::SND_UD | if fcb { control::FCB } else { 0 };
    Request::from_frame(&Frame::Long { c, a: address, ci, data }, timeout_for(address))
}

/// Select a slave by secondary address (`SND_UD` to address `FD`, CI `52`, FCB = 1 as
/// for every selection). A slave that matches answers `E5` and is then reached at
/// [`NETWORK_LAYER`] until deselected; every other slave deselects itself.
pub fn select(address: &SecondaryAddress) -> Request {
    snd_ud(NETWORK_LAYER, true, ci::SELECT, &address.to_bytes()).unwrap_or(Request::EMPTY)
}

/// Enhanced selection: also match the fabrication number (record `0C 78`, 8 BCD digits;
/// `None` wildcards it). For two meters that share an identification number. A slave that
/// does not support it does not answer.
pub fn select_enhanced(address: &SecondaryAddress, fabrication: Option<u32>) -> Request {
    let fab = fabrication.and_then(SecondaryAddress::id).map_or([0xFF; 4], |f| f.id_bytes());
    let [a, b, c, d, e, f, g, h] = address.to_bytes();
    let [f0, f1, f2, f3] = fab;
    snd_ud(NETWORK_LAYER, true, ci::SELECT, &[a, b, c, d, e, f, g, h, 0x0C, 0x78, f0, f1, f2, f3])
        .unwrap_or(Request::EMPTY)
}

/// Deselect: `SND_NKE` to the network-layer address.
pub fn deselect() -> Request {
    link_reset(NETWORK_LAYER)
}

/// Set a slave's primary address (`SND_UD`, CI `51`, record `01 7A new`). `None` for the
/// reserved addresses 251–255. Make sure first that nothing answers at `new`
/// ([`link_reset`] to it times out): two slaves on one address cannot be read.
pub fn set_primary_address(address: u8, new: u8) -> Option<Request> {
    (new <= 250).then(|| snd_ud(address, false, ci::DATA_SEND, &[0x01, 0x7A, new]).unwrap_or(Request::EMPTY))
}

/// Reset a slave's application layer (CI `50`), optionally only one telegram type (the
/// subcode: upper nibble the type, e.g. `1x` user data, lower nibble the subtelegram).
/// Also drops a readout selection back to the standard telegram.
pub fn application_reset(address: u8, subcode: Option<u8>) -> Request {
    let data = subcode.map(|s| [s]);
    snd_ud(address, false, ci::APPLICATION_RESET, data.as_ref().map_or(&[][..], |d| &d[..]))
        .unwrap_or(Request::EMPTY)
}

/// Ask for all data in the next `REQ_UD2` (global readout request, DIF `7F`).
pub fn request_all_data(address: u8) -> Request {
    snd_ud(address, false, ci::DATA_SEND, &[0x7F]).unwrap_or(Request::EMPTY)
}

/// Switch a slave to another baud rate (CI `B8`–`BF`). The slave acknowledges at the old
/// rate and listens at the new one.
pub fn set_baud(address: u8, baud: Baud) -> Request {
    snd_ud(address, false, baud.ci(), &[]).unwrap_or(Request::EMPTY)
}

/// Set a slave's clock (CI `51`, record `04 6D` date and time, type F).
pub fn set_time(address: u8, time: &DateTime) -> Request {
    let [a, b, c, d] = time.to_type_f();
    snd_ud(address, false, ci::DATA_SEND, &[0x04, 0x6D, a, b, c, d]).unwrap_or(Request::EMPTY)
}

/// Whom a readout reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A primary address (0–250).
    Primary(u8),
    /// A secondary address, selected first and deselected after.
    Secondary(SecondaryAddress),
}

/// What happened when a reply (or its absence) was handed to a state machine.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)] // returned by value once per reply; boxing needs alloc
pub enum Event<'a> {
    /// The bytes are the start of a frame: read more and call `reply` again with all of
    /// them.
    NeedMore,
    /// A telegram with records (for a readout), from the addressed meter.
    Telegram(Application<'a>),
    /// A meter was found (for a search).
    Found(Address),
    /// Several meters share this full identification number; select them with
    /// [`select_enhanced`] (fabrication number) or by manufacturer, version and medium.
    Ambiguous(SecondaryAddress),
    /// The slave acknowledged (`E5`).
    Acknowledged,
    /// No valid reply; the request will be repeated.
    Retrying,
    /// No valid reply after every retry: the slave does not answer.
    NoReply,
    /// A valid frame that cannot be decoded as a telegram.
    Invalid(Error),
    /// A meter acknowledged this selection (for a search), but its reply to the read
    /// did not name a meter that matches it: no reply, no long header, or another
    /// identification number.
    Unreadable(SecondaryAddress),
    /// A telegram from another meter than the readout expects (the identity that
    /// answered; `None` when the telegram carries none). Not delivered; the readout ends.
    WrongMeter(Option<Address>),
}

/// How a readout ended: see [`Readout::outcome`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The meter sent its last telegram (no "more records follow"), or answered `E5`
    /// instead of data: every record was read.
    Complete,
    /// The meter still has records, but [`Readout::max_telegrams`] telegrams were read.
    /// The telegrams delivered are only the first part of the meter's data.
    LimitReached,
    /// A reply was not a decodable telegram, or one of its records is broken (so
    /// whether more follow is unknown); [`Error::InvalidAddress`] for 251, 252 and 255.
    Invalid(Error),
    /// A telegram came from another meter than expected ([`Readout::expect_meter`], the
    /// selection of a secondary target, or the first telegram of this readout). It was
    /// not delivered. The identity is the one that answered (`None`: it sent none).
    WrongMeter(Option<Address>),
    /// No valid reply after every retry.
    NoReply,
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Complete => f.write_str("complete"),
            Self::LimitReached => f.write_str("telegram limit reached, the meter has more records"),
            Self::Invalid(e) => write!(f, "invalid telegram: {e}"),
            Self::WrongMeter(Some(a)) => write!(f, "another meter answered: {a}"),
            Self::WrongMeter(None) => f.write_str("another meter answered (without an identity)"),
            Self::NoReply => f.write_str("the meter does not answer"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadState {
    Reset,
    Select,
    Read,
    Deselect,
    Done,
}

/// Read every telegram of one meter: `SND_NKE` (primary) or selection (secondary), then
/// `REQ_UD2` with the frame count bit toggled until no more records follow, then
/// deselection (secondary). A request without a valid reply is repeated with the same
/// FCB, [`RETRIES`] times. When it is over, [`Readout::outcome`] says whether every
/// telegram was read, or why not.
///
/// # What counts as a reply
///
/// Hand [`Readout::reply`] the bytes of **exactly one frame**. On a line with noise or a
/// converter that echoes, cut the frame out of the receive buffer with [`wired::scan`]
/// (as [`serial::Bus`](crate::serial) does). Bytes after the frame make the reply garbled:
/// two slaves answered at once.
///
/// During a read, only a slave's response (`RSP_UD`, [`control::is_response`]) from the
/// requested primary address is a telegram (any address for 253 and 254, and for a
/// secondary target, which answers via 253). Anything else is handled like a garbled
/// reply: repeated, then [`Outcome::NoReply`]. Every telegram must also carry the expected
/// identity: the one given to [`Readout::expect_meter`], the selection of a secondary
/// target, and the identity of the first telegram; otherwise the readout ends with
/// [`Outcome::WrongMeter`].
///
/// These checks attribute data to a meter; they do not authenticate it. Wired M-Bus has
/// no authentication: any device on the line can send a frame with any address and
/// identity.
#[derive(Clone, Debug)]
pub struct Readout {
    target: Target,
    state: ReadState,
    fcb: bool,
    tries: u8,
    retries: u8,
    telegrams: u8,
    max_telegrams: u8,
    expect: Option<SecondaryAddress>,
    /// The identity of the first telegram (once `telegrams > 0`).
    first: Option<Address>,
    outcome: Option<Outcome>,
}

impl Readout {
    /// A readout with the default retries and a limit of 16 telegrams.
    pub const fn new(target: Target) -> Self {
        // Secondary: deselect every slave first (SND_NKE to FD), so no stale selection
        // answers along.
        let (state, outcome) = match target {
            Target::Primary(a @ (251 | 252 | BROADCAST)) => {
                (ReadState::Done, Some(Outcome::Invalid(Error::InvalidAddress(a))))
            }
            _ => (ReadState::Reset, None),
        };
        Self {
            target,
            state,
            fcb: true,
            tries: 0,
            retries: RETRIES,
            telegrams: 0,
            max_telegrams: 16,
            expect: None,
            first: None,
            outcome,
        }
    }

    /// Repeat a request at most `retries` times.
    #[must_use]
    pub const fn retries(mut self, retries: u8) -> Self {
        self.retries = retries;
        self
    }

    /// Read at most `max` telegrams (at least 1). A meter that still has records after
    /// them ends the readout with [`Outcome::LimitReached`], not [`Outcome::Complete`].
    #[must_use]
    pub const fn max_telegrams(mut self, max: u8) -> Self {
        self.max_telegrams = max;
        self
    }

    /// Accept only telegrams from this meter (the identity printed on it, wildcards
    /// allowed): e.g. bind a primary address to `SecondaryAddress::exact(&address)`. A
    /// telegram with another identity, or none, ends the readout with
    /// [`Outcome::WrongMeter`].
    #[must_use]
    pub const fn expect_meter(mut self, meter: SecondaryAddress) -> Self {
        self.expect = Some(meter);
        self
    }

    const fn address(&self) -> u8 {
        match self.target {
            Target::Primary(a) => a,
            Target::Secondary(_) => NETWORK_LAYER,
        }
    }

    /// The next frame to send, or `None` when the readout is over.
    pub fn next_request(&mut self) -> Option<Request> {
        Some(match (self.state, self.target) {
            // Primary: SND_NKE to the slave. Secondary: SND_NKE to FD, deselecting all.
            (ReadState::Reset, _) => link_reset(self.address()),
            (ReadState::Select, Target::Secondary(s)) => select(&s),
            (ReadState::Read, _) => {
                Request::from_frame(&wired::req_ud2(self.address(), self.fcb), timeout_for(0)).ok()?
            }
            (ReadState::Deselect, _) => deselect(),
            _ => return None,
        })
    }

    /// How the readout ended; `None` while it runs (including the deselection).
    pub const fn outcome(&self) -> Option<Outcome> {
        match self.state {
            ReadState::Done => self.outcome,
            _ => None,
        }
    }

    /// Whether every telegram was read: [`Outcome::Complete`].
    pub const fn is_complete(&self) -> bool {
        matches!(self.outcome(), Some(Outcome::Complete))
    }

    /// How many telegrams were delivered.
    pub const fn telegrams(&self) -> u8 {
        self.telegrams
    }

    const fn advance(&mut self, next: ReadState) {
        self.state = next;
        self.tries = 0;
    }

    /// End the readout; a secondary target that may be selected is deselected first.
    const fn finish(&mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
        let next = match (self.target, self.state) {
            (Target::Secondary(_), ReadState::Select | ReadState::Read) => ReadState::Deselect,
            _ => ReadState::Done,
        };
        self.advance(next);
    }

    /// The timeout passed without a valid reply (`received` is ignored: a readout
    /// repeats the request either way).
    pub fn timeout(&mut self, received: &[u8]) -> Event<'static> {
        let _ = received;
        match (self.state, self.target) {
            // Unselected slaves need not answer the deselection: go on to the selection.
            (ReadState::Reset, Target::Secondary(_)) => {
                self.advance(ReadState::Select);
                return Event::NoReply;
            }
            // Not every slave acknowledges a deselection; the outcome stands.
            (ReadState::Deselect, _) => {
                self.advance(ReadState::Done);
                return Event::NoReply;
            }
            (ReadState::Done, _) => return Event::NoReply,
            _ => {}
        }
        if self.tries < self.retries {
            self.tries += 1;
            Event::Retrying
        } else {
            self.finish(Outcome::NoReply);
            Event::NoReply
        }
    }

    /// Hand over what arrived: the bytes of one frame (see [the rules](Readout#what-counts-as-a-reply)).
    pub fn reply<'a>(&mut self, frame: &'a [u8]) -> Event<'a> {
        let (parsed, used) = match Frame::parse(frame) {
            Ok(None) => return Event::NeedMore,
            Ok(Some(parsed)) => parsed,
            Err(_) => return self.timeout(frame),
        };
        if self.state == ReadState::Deselect {
            self.advance(ReadState::Done);
            return Event::Acknowledged;
        }
        if used != frame.len() {
            // Bytes after the frame: two answers overlapped.
            return self.timeout(frame);
        }
        match (self.state, parsed) {
            (ReadState::Reset, Frame::Ack) => {
                // EN 13757-2: after SND_NKE the next REQ_UD2 has FCB set.
                self.fcb = true;
                let next = match self.target {
                    Target::Primary(_) => ReadState::Read,
                    Target::Secondary(_) => ReadState::Select,
                };
                self.advance(next);
                Event::Acknowledged
            }
            (ReadState::Select, Frame::Ack) => {
                self.fcb = true;
                self.advance(ReadState::Read);
                Event::Acknowledged
            }
            // E5 instead of the next page: the slave has nothing more to send. Before the
            // first page, an E5 is a late or doubled acknowledgement, not data.
            (ReadState::Read, Frame::Ack) if self.telegrams > 0 => {
                self.finish(Outcome::Complete);
                Event::Acknowledged
            }
            (ReadState::Read, Frame::Long { c, a, ci, data }) if self.answers(c, a) => {
                self.telegram(ci, data)
            }
            _ => self.timeout(frame),
        }
    }

    /// A slave's response from the address asked.
    const fn answers(&self, c: u8, a: u8) -> bool {
        control::is_response(c)
            && match self.target {
                Target::Primary(p) if p <= 250 => a == p,
                _ => true,
            }
    }

    fn telegram<'a>(&mut self, ci: u8, data: &'a [u8]) -> Event<'a> {
        let app = match Application::parse(ci, data) {
            Ok(app) => app,
            Err(e) => {
                self.finish(Outcome::Invalid(e));
                return Event::Invalid(e);
            }
        };
        let meter = app.meter();
        let selection = match self.target {
            Target::Secondary(s) => Some(s),
            Target::Primary(_) => None,
        };
        let fits = |p: &SecondaryAddress| *p == SecondaryAddress::ANY || meter.is_some_and(|m| p.matches(&m));
        let same = self.telegrams == 0 || self.first == meter;
        if !(selection.iter().chain(&self.expect).all(fits) && same) {
            self.finish(Outcome::WrongMeter(meter));
            return Event::WrongMeter(meter);
        }
        self.first = meter;
        self.telegrams = self.telegrams.saturating_add(1);
        self.fcb = !self.fcb;
        match app.records().map(more_follows) {
            // A broken record hides whether `1F` follows: not complete.
            Some(Err(e)) => self.finish(Outcome::Invalid(e)),
            None | Some(Ok(false)) => self.finish(Outcome::Complete),
            Some(Ok(true)) if self.telegrams >= self.max_telegrams.max(1) => {
                self.finish(Outcome::LimitReached);
            }
            Some(Ok(true)) => self.advance(ReadState::Read),
        }
        Event::Telegram(app)
    }
}

/// Whether a telegram's records end with `1F` (more records follow); the first broken
/// record otherwise.
#[allow(clippy::needless_pass_by_value)] // used as `.map(more_follows)`
fn more_follows(mut records: crate::Records<'_>) -> Result<bool> {
    for r in records.by_ref() {
        r?;
    }
    Ok(records.more_follows())
}

/// A secondary-address search (wildcard scan): finds every meter on the bus.
///
/// Probes a selection with the first digit fixed and the rest wildcarded. No reply: no
/// meter there. `E5`: exactly one meter, read with `REQ_UD2` to learn its address. A
/// garbled reply: several meters collided, so the next digit is fixed too, 0 to 9, and
/// so on. Depth-first, at most 8 digits deep; ends after at most `10 × 8 × (meters + 1)`
/// selections, plus one read per meter.
#[derive(Clone, Debug)]
pub struct Search {
    prefix: [u8; 8],
    depth: u8,
    reading: bool,
    done: bool,
    tries: u8,
    retries: u8,
    selections: u32,
    max_selections: u32,
    exhausted: bool,
}

impl Default for Search {
    fn default() -> Self {
        Self::new()
    }
}

impl Search {
    /// A search of the whole bus.
    pub const fn new() -> Self {
        Self {
            prefix: [0; 8],
            depth: 1,
            reading: false,
            done: false,
            tries: 0,
            retries: RETRIES,
            selections: 0,
            max_selections: 25_000,
            exhausted: false,
        }
    }

    /// Give up after `max` selections (default 25 000: a bus of 250 meters needs at most
    /// 10 × 8 × 251 = 20 080). A stuck line or a babbling device makes every probe look like a
    /// collision, and the full tree has 10⁸ prefixes.
    #[must_use]
    pub const fn max_selections(mut self, max: u32) -> Self {
        self.max_selections = max;
        self
    }

    /// Whether the search covered the whole bus (`false` while running, or after it gave
    /// up at [`Search::max_selections`]).
    pub const fn is_complete(&self) -> bool {
        self.done && !self.exhausted
    }

    /// The selection pattern being probed.
    pub fn probe(&self) -> SecondaryAddress {
        let mut s = SecondaryAddress::ANY;
        for (slot, digit) in s.digits.iter_mut().zip(self.prefix).take(usize::from(self.depth)) {
            *slot = Some(digit);
        }
        s
    }

    /// How many selections have been sent.
    pub const fn selections(&self) -> u32 {
        self.selections
    }

    /// The next frame to send, or `None` when the search is over.
    pub fn next_request(&mut self) -> Option<Request> {
        if self.done {
            return None;
        }
        if self.reading {
            return Request::from_frame(&wired::req_ud2(NETWORK_LAYER, true), timeout_for(0)).ok();
        }
        if self.tries == 0 {
            if self.selections >= self.max_selections {
                self.done = true;
                self.exhausted = true;
                return None;
            }
            self.selections = self.selections.saturating_add(1);
        }
        Some(select(&self.probe()))
    }

    /// Move to the next prefix at this depth, climbing up when a digit's 0–9 are done.
    fn next_sibling(&mut self) {
        self.tries = 0;
        self.reading = false;
        loop {
            let Some(d) = self.prefix.get_mut(usize::from(self.depth) - 1) else {
                self.done = true;
                return;
            };
            if *d < 9 {
                *d += 1;
                return;
            }
            *d = 0;
            if self.depth == 1 {
                self.done = true;
                return;
            }
            self.depth -= 1;
        }
    }

    /// The timeout passed. `received` is whatever arrived in that time (empty if
    /// nothing): bytes that are not a valid frame mean several meters answered at once.
    pub fn timeout(&mut self, received: &[u8]) -> Event<'static> {
        if !received.is_empty() {
            return self.collision();
        }
        if self.tries < self.retries {
            self.tries += 1;
            return Event::Retrying;
        }
        if self.reading {
            // The selection was acknowledged, the read never was: report, don't drop.
            return self.unreadable();
        }
        // No meter under this prefix.
        self.next_sibling();
        Event::NoReply
    }

    fn unreadable(&mut self) -> Event<'static> {
        let probe = self.probe();
        self.next_sibling();
        Event::Unreadable(probe)
    }

    /// Several meters answered: fix one more digit, starting at 0.
    fn collision(&mut self) -> Event<'static> {
        self.reading = false;
        self.tries = 0;
        if self.depth >= 8 {
            // A full ID collides: two meters with one number. Report it and go on.
            let probe = self.probe();
            self.next_sibling();
            return Event::Ambiguous(probe);
        }
        self.depth += 1;
        if let Some(d) = self.prefix.get_mut(usize::from(self.depth) - 1) {
            *d = 0;
        }
        Event::Retrying
    }

    /// Hand over what arrived. Incomplete but plausible bytes: [`Event::NeedMore`];
    /// anything invalid: a collision.
    pub fn reply(&mut self, bytes: &[u8]) -> Event<'static> {
        match Frame::parse(bytes) {
            Ok(None) => Event::NeedMore,
            // Bytes after a frame: a second answer overlapping the first.
            Ok(Some((_, used))) if used < bytes.len() => self.collision(),
            Ok(Some((Frame::Ack, _))) if !self.reading => {
                self.reading = true;
                self.tries = 0;
                Event::Acknowledged
            }
            // E5 to the read: the selected meter answered, but not with data.
            Ok(Some((Frame::Ack, _))) => self.timeout(&[]),
            Ok(Some((Frame::Long { c, ci, data, .. }, _))) if self.reading && control::is_response(c) => {
                let probe = self.probe();
                match Application::parse(ci, data).ok().and_then(|a| a.meter()) {
                    Some(meter) if probe.matches(&meter) => {
                        self.next_sibling();
                        Event::Found(meter)
                    }
                    _ => self.unreadable(),
                }
            }
            // Garbled: several meters answered.
            _ => self.collision(),
        }
    }
}
