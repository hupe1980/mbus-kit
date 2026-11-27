//! Wireless M-Bus receiver sticks, sans-I/O: their serial protocols in, radio telegrams
//! out.
//!
//! | Stick | Serial | Framing | Telegram | RSSI |
//! |---|---|---|---|---|
//! | IMST iM871A, iM170A | 57600 8N1 | `A5`, control/endpoint, message, length, payload, (timestamp), (RSSI), CRC-16/X-25 | L field = HCI length, then C … | raw, approximate dBm |
//! | IMST iU891A, iM891A | 115200 8N1 | SLIP; SAP, message, payload, CRC-16/X-25 | L field first | dBm (signed) |
//! | Amber/Würth AMB8465, AMB8665 (command mode) | 9600 8N1 | `FF`, command, length, payload, (RSSI), XOR | L field = length − 1 (with RSSI), then C … | dBm |
//!
//! Every stick checks and removes the link-layer CRCs: a [`Received`] frame starts at the
//! L field and goes to [`wireless::decode`](crate::wireless::decode) or
//! [`wireless::open`](crate::wireless::open) with
//! [`Format::Stripped`](crate::wireless::Format::Stripped).
//!
//! ```
//! use mbus_kit::stick::{LinkMode, Message, Receiver, Stick};
//!
//! let stick = Stick::Im871a;
//! let listen = stick.listen(LinkMode::T1).unwrap(); // write listen.bytes() to the port once
//! let mut rx = Receiver::new(stick);
//! # let from_port = [0xA5u8, 0xC2, 0x03, 0x19, 0x44, 0x24, 0x23, 0x85, 0x07, 0x60, 0x14, 0x40, 0x18, 0xA1, 0x51, 0x49, 0x00, 0x13, 0x53, 0x55, 0xB8, 0x53, 0x18, 0x92, 0xDF, 0x2B, 0x27, 0x37, 0x34, 0x56, 0x60, 0xEB];
//! rx.extend(&from_port); // whatever the port delivered, in any chunks
//! while let Some(message) = rx.next_message() {
//!     match message {
//!         Ok(Message::Telegram(t)) => println!("{:02X?} at {:?} dBm", t.frame, t.rssi_dbm),
//!         Ok(Message::Response { .. }) => { /* a confirmation of a command */ }
//!         Err(e) => eprintln!("skipped: {e}"), // noise or a corrupted stick frame
//!     }
//! }
//! ```
//!
//! The Amber sticks deliver telegrams framed only in command mode with RSSI: run
//! [`Stick::setup`] once (it writes the stick's non-volatile settings), then
//! [`Stick::listen`] after every power-up.

use core::fmt;

use crate::crc::crc16_x25;
use crate::{Error, Result};

/// A supported receiver stick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    /// IMST iM871A (868 MHz) and iM170A (169 MHz): WM-Bus HCI 1.x.
    Im871a,
    /// IMST iU891A, iM891A, iU893A: the WM-Bus gateway HCI 2.x over SLIP.
    Iu891a,
    /// Amber/Würth AMB8465 (Metis-I) and AMB8665 (Metis-II), in command mode with RSSI.
    Amb8465,
}

/// A link mode to receive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkMode {
    /// S1 (stationary, 32.768 kcps).
    S1,
    /// T1 (frequent transmit, the most common for water and heat meters).
    T1,
    /// C1 (compact, formats A and B).
    C1,
    /// C1 and T1 at once.
    C1T1,
}

impl fmt::Display for LinkMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::S1 => "S1",
            Self::T1 => "T1",
            Self::C1 => "C1",
            Self::C1T1 => "C1+T1",
        })
    }
}

/// Bytes to write to a stick.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Command {
    bytes: [u8; 32],
    len: u8,
}

impl Command {
    /// The bytes to write.
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }

    fn new() -> Self {
        Self { bytes: [0; 32], len: 0 }
    }

    fn push(&mut self, b: u8) {
        if let Some(slot) = self.bytes.get_mut(usize::from(self.len)) {
            *slot = b;
            self.len += 1;
        }
    }

    /// An IMST HCI 1.x frame with CRC: `A5`, control 8 | endpoint, message, length.
    fn imst(endpoint: u8, msg: u8, payload: &[u8]) -> Self {
        let mut c = Self::new();
        c.push(0xA5);
        c.push(0x80 | endpoint);
        c.push(msg);
        c.push(u8::try_from(payload.len()).unwrap_or(0));
        for &b in payload {
            c.push(b);
        }
        let [lo, hi] = crc16_x25(c.bytes().get(1..).unwrap_or(&[])).to_le_bytes();
        c.push(lo);
        c.push(hi);
        c
    }

    /// An IMST gateway HCI 2.x message, SLIP-framed: `C0`, SAP, message, payload, CRC, `C0`.
    fn slip(sap: u8, msg: u8, payload: &[u8]) -> Self {
        let mut body = [0u8; 16];
        let mut n = 0usize;
        for &b in [sap, msg].iter().chain(payload) {
            if let Some(slot) = body.get_mut(n) {
                *slot = b;
                n += 1;
            }
        }
        let [lo, hi] = crc16_x25(body.get(..n).unwrap_or(&[])).to_le_bytes();
        let mut c = Self::new();
        c.push(SLIP_END);
        for &b in body.get(..n).unwrap_or(&[]).iter().chain(&[lo, hi]) {
            match b {
                SLIP_END => [SLIP_ESC, 0xDC].iter().for_each(|&e| c.push(e)),
                SLIP_ESC => [SLIP_ESC, 0xDD].iter().for_each(|&e| c.push(e)),
                b => c.push(b),
            }
        }
        c.push(SLIP_END);
        c
    }

    /// An Amber command: `FF`, command, length, payload, XOR of everything before.
    fn amber(cmd: u8, payload: &[u8]) -> Self {
        let mut c = Self::new();
        c.push(0xFF);
        c.push(cmd);
        c.push(u8::try_from(payload.len()).unwrap_or(0));
        for &b in payload {
            c.push(b);
        }
        let cs = c.bytes().iter().fold(0u8, |x, &b| x ^ b);
        c.push(cs);
        c
    }
}

impl fmt::Debug for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Command({:02X?})", self.bytes())
    }
}

const SLIP_END: u8 = 0xC0;
const SLIP_ESC: u8 = 0xDB;

impl Stick {
    /// The serial speed the stick uses out of the box (8N1).
    pub const fn baud(self) -> u32 {
        match self {
            Self::Im871a => 57_600,
            Self::Iu891a => 115_200,
            Self::Amb8465 => 9_600,
        }
    }

    /// The command that starts reception of `mode`, until the stick is powered off.
    /// `None` where the stick cannot receive that mode (S1 on the Amber sticks).
    ///
    /// | Stick | Command | Modes |
    /// |---|---|---|
    /// | iM871A | `SET_CONFIG_REQ`, RAM only, link mode and RSSI on | S1 `00`, T1 `03`, C1 `07` (C1 format B also receives A), C1+T1 `0A` (firmware ≥ 1.4) |
    /// | iU891A | Set Active Configuration (RAM): mode, receive notification, recalibration | S `01`, T `02`, C `05`, C/T `03` |
    /// | AMB8465 | `CMD_SET_MODE_REQ` (RAM) | T2_other `08`, C2_other `0E`, C2_T2_other `09` |
    pub fn listen(self, mode: LinkMode) -> Option<Command> {
        Some(match self {
            Self::Im871a => {
                let code = match mode {
                    LinkMode::S1 => 0x00,
                    LinkMode::T1 => 0x03,
                    LinkMode::C1 => 0x07,
                    LinkMode::C1T1 => 0x0A,
                };
                // NVM no; IIFlag1: link mode; IIFlag2: AutoRSSI on.
                Command::imst(0x01, 0x03, &[0x00, 0x02, code, 0x10, 0x01])
            }
            Self::Iu891a => {
                let code = match mode {
                    LinkMode::S1 => 0x01,
                    LinkMode::T1 => 0x02,
                    LinkMode::C1 => 0x05,
                    LinkMode::C1T1 => 0x03,
                };
                // Link mode; options: receive notification + radio recalibration; UI
                // options: none; LED flash 50 ms (the default); recalibration every 60 s.
                let [l0, l1] = 50u16.to_le_bytes();
                let [r0, r1, r2, r3] = 60_000u32.to_le_bytes();
                Command::slip(0x09, 0x03, &[code, 0x0A, 0x00, 0x00, 0x00, l0, l1, r0, r1, r2, r3])
            }
            Self::Amb8465 => {
                let code = match mode {
                    LinkMode::S1 => return None,
                    LinkMode::T1 => 0x08,
                    LinkMode::C1 => 0x0E,
                    LinkMode::C1T1 => 0x09,
                };
                Command::amber(0x04, &[code])
            }
        })
    }

    /// A harmless request whose response shows the stick is there: `PING_REQ` (iM871A,
    /// iU891A) or `CMD_FWV_REQ` (Amber).
    pub fn ping(self) -> Command {
        match self {
            Self::Im871a => Command::imst(0x01, 0x01, &[]),
            Self::Iu891a => Command::slip(0x01, 0x01, &[]),
            Self::Amb8465 => Command::amber(0x0C, &[]),
        }
    }

    /// One-time preparation, written to the stick's non-volatile memory: for the Amber
    /// sticks, frame received telegrams as `CMD_DATA_IND` (`UART_CMD_Out_Enable` = 1),
    /// append the RSSI (`RSSI_Enable` = 1), then reset. Empty for the IMST sticks.
    /// Flash memory wears: run it once per stick, not on every start.
    pub fn setup(self) -> impl Iterator<Item = Command> {
        let amber = matches!(self, Self::Amb8465);
        [
            Command::amber(0x09, &[0x05, 0x01, 0x01]),
            Command::amber(0x09, &[0x45, 0x01, 0x01]),
            Command::amber(0x05, &[]),
        ]
        .into_iter()
        .filter(move |_| amber)
    }
}

/// A radio telegram the stick received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Received<'a> {
    /// The telegram from the L field, link-layer CRCs removed (`Format::Stripped`).
    pub frame: &'a [u8],
    /// The signal strength in dBm: as reported (iU891A), by the vendor's formula
    /// (Amber), or approximated from the raw value (iM871A, whose vendor gives none).
    pub rssi_dbm: Option<i16>,
    /// The stick's raw RSSI byte, if it sent one.
    pub rssi_raw: Option<u8>,
    /// The stick's reception timestamp: its own clock (iM871A), or UTC seconds (iU891A).
    pub timestamp: Option<u32>,
    /// The link mode and frame format it arrived in, if the stick says (iU891A).
    pub mode: Option<(LinkMode, bool)>,
    /// The stick decrypted the payload itself with a key stored in it (iU891A).
    pub decrypted_by_stick: bool,
}

/// What a stick sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message<'a> {
    /// A received radio telegram.
    Telegram(Received<'a>),
    /// A response or indication other than a telegram: `group` is the IMST endpoint or
    /// SAP (0 for Amber), `id` the message or command (Amber confirmations are
    /// `command | 0x80`), `payload` its bytes (a status byte first on the iU891A).
    Response {
        /// Endpoint (iM871A), SAP (iU891A) or 0 (Amber).
        group: u8,
        /// Message id or command.
        id: u8,
        /// The payload.
        payload: &'a [u8],
    },
}

const CAPACITY: usize = 640;

/// Turns a stick's byte stream into [`Message`]s, without allocating.
///
/// Push bytes with [`Receiver::extend`] in whatever chunks the port delivers, then call
/// [`Receiver::next_message`] until it returns `None`. Noise and corrupted stick frames
/// come out as errors and are skipped; the next valid frame always decodes.
#[derive(Clone)]
pub struct Receiver {
    stick: Stick,
    buf: [u8; CAPACITY],
    len: usize,
    /// Bytes at the front that the last message used; dropped on the next call.
    used: usize,
}

#[allow(clippy::missing_fields_in_debug)] // the buffer is noise in a log
impl fmt::Debug for Receiver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Receiver")
            .field("stick", &self.stick)
            .field("buffered", &(self.len - self.used))
            .finish()
    }
}

impl Receiver {
    /// A receiver for `stick`.
    pub const fn new(stick: Stick) -> Self {
        Self { stick, buf: [0; CAPACITY], len: 0, used: 0 }
    }

    /// Add bytes from the port. Returns how many were taken: fewer than given only when
    /// the buffer is full of an unfinished frame; call [`Receiver::next_message`] and
    /// extend with the rest.
    pub fn extend(&mut self, bytes: &[u8]) -> usize {
        self.compact();
        let free = CAPACITY - self.len;
        let n = bytes.len().min(free);
        if let (Some(dst), Some(src)) = (self.buf.get_mut(self.len..self.len + n), bytes.get(..n)) {
            dst.copy_from_slice(src);
            self.len += n;
        }
        n
    }

    /// The bytes received and not yet used.
    fn data(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    fn compact(&mut self) {
        if self.used > 0 {
            self.buf.copy_within(self.used..self.len, 0);
            self.len -= self.used;
            self.used = 0;
        }
    }

    /// The next message, an error for bytes that were skipped, or `None` until more
    /// bytes arrive.
    pub fn next_message(&mut self) -> Option<Result<Message<'_>>> {
        self.compact();
        // A full buffer that holds no complete frame: drop it rather than stall.
        let r = match self.stick {
            Stick::Im871a => self.imst(),
            Stick::Iu891a => self.gateway(),
            Stick::Amb8465 => self.amber(),
        };
        match r {
            Step::Wait if self.len == CAPACITY => {
                self.used = self.len;
                Some(Err(Error::InvalidLength))
            }
            Step::Wait => None,
            Step::Skip(n, e) => {
                self.used = n.max(1).min(self.len);
                Some(Err(e))
            }
            Step::Done(n, kind) => {
                self.used = n;
                Some(Ok(self.message(kind)))
            }
        }
    }

    /// Skip to the first `start` byte; `true` if one is at the front.
    fn seek(&mut self, start: u8) -> bool {
        let skip = self.data().get(..).and_then(|b| b.iter().position(|&x| x == start)).unwrap_or(self.len);
        if skip > 0 {
            self.used = skip;
            self.compact();
        }
        self.len > 0
    }

    fn imst(&mut self) -> Step {
        if !self.seek(0xA5) {
            return Step::Wait;
        }
        let Some(&[_, ctrl, msg, len]) = self.data().get(..4).and_then(|b| <&[u8; 4]>::try_from(b).ok())
        else {
            return Step::Wait;
        };
        if ctrl & 0x10 != 0 || ctrl.trailing_zeros() >= 4 {
            return Step::Skip(1, Error::InvalidHeader);
        }
        let (ts, rssi, crc) = (ctrl & 0x20 != 0, ctrl & 0x40 != 0, ctrl & 0x80 != 0);
        let body = 4 + usize::from(len);
        let total = body + if ts { 4 } else { 0 } + usize::from(rssi) + if crc { 2 } else { 0 };
        let Some(frame) = self.data().get(..total) else { return Step::Wait };
        if crc {
            let (data, fcs) = frame.split_at(total - 2);
            let computed = crc16_x25(data.get(1..).unwrap_or(&[]));
            let found = fcs.first().zip(fcs.get(1)).map_or(0, |(&lo, &hi)| u16::from_le_bytes([lo, hi]));
            if computed != found {
                return Step::Skip(1, Error::Crc { block: 0, computed, found });
            }
        }
        let timestamp = ts.then(|| self.u32_le(body)).flatten();
        let raw = rssi.then(|| self.data().get(body + if ts { 4 } else { 0 }).copied()).flatten();
        if ctrl & 0x0F == 0x02 && msg == 0x03 {
            // RADIOLINK WMBUSMSG_IND: the HCI length byte is the telegram's L field.
            let rssi_dbm = raw.map(|r| i16::try_from((8 * i32::from(r) - 1900) / 15).unwrap_or(i16::MIN));
            let t = Kind::Telegram {
                at: 3,
                len: 1 + usize::from(len),
                rssi_dbm,
                raw,
                timestamp,
                mode: None,
                decrypted: false,
            };
            return Step::Done(total, t);
        }
        Step::Done(total, Kind::Response { group: ctrl & 0x0F, id: msg, at: 4, len: usize::from(len) })
    }

    fn gateway(&mut self) -> Step {
        // SLIP: drop delimiters in front, then wait for the end delimiter.
        while self.data().get(..).and_then(<[u8]>::first) == Some(&SLIP_END) {
            self.used = 1;
            self.compact();
        }
        let Some(end) = self.data().get(..).and_then(|b| b.iter().position(|&x| x == SLIP_END)) else {
            return Step::Wait;
        };
        // Unescape in place: the result is never longer than the input.
        let mut w = 0usize;
        let mut r = 0usize;
        while r < end {
            let b = self.data().get(r).copied().unwrap_or(0);
            let out = if b == SLIP_ESC {
                r += 1;
                match self.data().get(r) {
                    Some(0xDC) => SLIP_END,
                    Some(0xDD) => SLIP_ESC,
                    _ => return Step::Skip(end, Error::InvalidHeader),
                }
            } else {
                b
            };
            if let Some(slot) = self.buf.get_mut(w) {
                *slot = out;
            }
            w += 1;
            r += 1;
        }
        if w < 4 {
            return Step::Skip(end, Error::UnexpectedEnd);
        }
        let Some(body) = self.data().get(..w) else { return Step::Skip(end, Error::UnexpectedEnd) };
        let (data, fcs) = body.split_at(w - 2);
        let computed = crc16_x25(data);
        let found = fcs.first().zip(fcs.get(1)).map_or(0, |(&lo, &hi)| u16::from_le_bytes([lo, hi]));
        if computed != found {
            return Step::Skip(end, Error::Crc { block: 0, computed, found });
        }
        let (sap, msg) = (data.first().copied().unwrap_or(0), data.get(1).copied().unwrap_or(0));
        let payload = w - 4;
        if sap == 0x09 && msg == 0x20 && payload > 8 {
            // Rx Message Ind: time (4, LSB first), decryption status, encryption mode,
            // packet info, RSSI (dBm, signed), packet from the L field.
            let timestamp = self.u32_le(2);
            let decrypted = self.data().get(6) == Some(&1);
            let info = self.data().get(8).copied().unwrap_or(0);
            let raw = self.data().get(9).copied();
            let mode = match info {
                1 => Some((LinkMode::S1, false)),
                2 | 6 | 7 => Some((LinkMode::T1, false)),
                4 | 5 | 8 => Some((LinkMode::C1, false)),
                20 | 21 | 24 => Some((LinkMode::C1, true)),
                _ => None,
            };
            let rssi_dbm = raw.map(|r| i16::from(i8::from_le_bytes([r])));
            let t = Kind::Telegram { at: 10, len: payload - 8, rssi_dbm, raw, timestamp, mode, decrypted };
            return Step::Done(end, t);
        }
        Step::Done(end, Kind::Response { group: sap, id: msg, at: 2, len: payload })
    }

    fn amber(&mut self) -> Step {
        if !self.seek(0xFF) {
            return Step::Wait;
        }
        let Some(&[_, cmd, len]) = self.data().get(..3).and_then(|b| <&[u8; 3]>::try_from(b).ok()) else {
            return Step::Wait;
        };
        // Only a data indication or a confirmation (command | 80) comes from the stick.
        if cmd != 0x03 && cmd & 0x80 == 0 {
            return Step::Skip(1, Error::InvalidHeader);
        }
        let total = 4 + usize::from(len);
        let Some(frame) = self.data().get(..total) else { return Step::Wait };
        let (data, cs) = frame.split_at(total - 1);
        let computed = data.iter().fold(0u8, |x, &b| x ^ b);
        let found = cs.first().copied().unwrap_or(0);
        if computed != found {
            return Step::Skip(1, Error::Checksum { computed, found });
        }
        if cmd == 0x03 && len >= 2 {
            // CMD_DATA_IND: length = L + 1 with the RSSI byte last. Restore the L field.
            let raw = self.data().get(total - 2).copied();
            if let Some(l) = self.buf.get_mut(2) {
                *l = len - 1;
            }
            let rssi_dbm = raw.map(|r| i16::from(i8::from_le_bytes([r])) / 2 - 74);
            let t = Kind::Telegram {
                at: 2,
                len: usize::from(len),
                rssi_dbm,
                raw,
                timestamp: None,
                mode: None,
                decrypted: false,
            };
            return Step::Done(total, t);
        }
        Step::Done(total, Kind::Response { group: 0, id: cmd, at: 3, len: usize::from(len) })
    }

    fn u32_le(&self, at: usize) -> Option<u32> {
        let b = self.data().get(at..at + 4)?;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    }

    fn message(&self, kind: Kind) -> Message<'_> {
        match kind {
            Kind::Telegram { at, len, rssi_dbm, raw, timestamp, mode, decrypted } => {
                Message::Telegram(Received {
                    frame: self.data().get(at..at + len).unwrap_or(&[]),
                    rssi_dbm,
                    rssi_raw: raw,
                    timestamp,
                    mode,
                    decrypted_by_stick: decrypted,
                })
            }
            Kind::Response { group, id, at, len } => {
                Message::Response { group, id, payload: self.data().get(at..at + len).unwrap_or(&[]) }
            }
        }
    }
}

enum Step {
    Wait,
    Skip(usize, Error),
    Done(usize, Kind),
}

#[derive(Clone, Copy)]
enum Kind {
    Telegram {
        at: usize,
        len: usize,
        rssi_dbm: Option<i16>,
        raw: Option<u8>,
        timestamp: Option<u32>,
        mode: Option<(LinkMode, bool)>,
        decrypted: bool,
    },
    Response {
        group: u8,
        id: u8,
        at: usize,
        len: usize,
    },
}
