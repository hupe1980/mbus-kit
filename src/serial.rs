//! A blocking wired master over a real port (feature `std`): the sans-I/O state machines
//! of [`master`](crate::master), driven over anything that is `Read + Write`.
//!
//! That is a serial device of an M-Bus level converter (opened at 8E1 with a **short read
//! timeout**, e.g. 10 ms, by any serial crate or `stty`), a `TcpStream` to an Ethernet
//! M-Bus gateway, or a test double. The driver writes each request, reads until the state
//! machine has a complete frame or the response timeout has passed, removes a converter's
//! echo of the request, and keeps collecting while bytes still arrive (a long frame at
//! 2400 Bd takes more than a second).
//!
//! ```no_run
//! # fn open() -> std::net::TcpStream { unimplemented!() }
//! use mbus_kit::master::{Baud, Target};
//! use mbus_kit::serial::Bus;
//!
//! let mut bus = Bus::new(open(), Baud::B2400);
//! for telegram in bus.readout(Target::Primary(1))? {
//!     for record in telegram.application()?.records().into_iter().flatten() {
//!         println!("{}", record?);
//!     }
//! }
//! # Ok::<(), mbus_kit::serial::Error>(())
//! ```
//!
//! The port must not block forever on a read: give it a read timeout, or reads that
//! return `TimedOut`, `WouldBlock` or `Ok(0)` when nothing arrived.

extern crate std;

use std::fmt;
use std::io::{self, ErrorKind, Read, Write};
use std::time::{Duration, Instant};
use std::vec::Vec;

use crate::app::{Address, Application};
use crate::master::{Baud, Event, Outcome, Readout, Request, Search, SecondaryAddress, Target};
use crate::wired::Frame;

/// What can go wrong on a real bus.
#[derive(Debug)]
pub enum Error {
    /// The port failed.
    Io(io::Error),
    /// The readout ended before the meter's data did: `outcome` says why (the meter does
    /// not answer, the telegram limit, an undecodable telegram, another meter).
    /// `telegrams` holds what was read before, which is only part of the meter's data.
    Incomplete {
        /// Why the readout is not complete; never [`Outcome::Complete`].
        outcome: Outcome,
        /// The telegrams read before it ended.
        telegrams: Vec<Telegram>,
    },
    /// A telegram could not be decoded.
    Invalid(crate::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "port: {e}"),
            Self::Incomplete { outcome, telegrams } => {
                write!(f, "incomplete readout after {} telegram(s): {outcome}", telegrams.len())
            }
            Self::Invalid(e) => write!(f, "invalid reply: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Invalid(e) => Some(e),
            Self::Incomplete { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Self::Invalid(e)
    }
}

/// One telegram of a readout, as received (a long frame).
#[derive(Clone, PartialEq, Eq)]
pub struct Telegram {
    frame: Vec<u8>,
}

impl Telegram {
    /// The frame bytes, from `68` to `16`.
    pub fn bytes(&self) -> &[u8] {
        &self.frame
    }

    /// The application layer: header and records.
    pub fn application(&self) -> crate::Result<Application<'_>> {
        match Frame::parse(&self.frame)? {
            Some((Frame::Long { ci, data, .. }, _)) => Application::parse(ci, data),
            _ => Err(crate::Error::InvalidHeader),
        }
    }
}

impl fmt::Debug for Telegram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Telegram({:02X?})", self.frame)
    }
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// Every meter with a unique secondary address.
    pub meters: Vec<Address>,
    /// Identification numbers several meters share (see [`master::Event::Ambiguous`](crate::master::Event::Ambiguous)).
    pub ambiguous: Vec<SecondaryAddress>,
    /// Selections a meter acknowledged without then naming itself (see
    /// [`master::Event::Unreadable`](crate::master::Event::Unreadable)).
    pub unreadable: Vec<SecondaryAddress>,
    /// Whether the whole bus was covered; `false` when the search gave up at its
    /// selection limit (a stuck line, a babbling device).
    pub complete: bool,
}

/// A wired M-Bus behind a port.
#[derive(Debug)]
pub struct Bus<P> {
    port: P,
    baud: Baud,
    slack: Duration,
    rx: Vec<u8>,
}

/// After the last byte, how long the line must stay quiet before a reply is complete:
/// generous for USB converters that deliver in chunks (FTDI latency timer 16 ms).
const IDLE: Duration = Duration::from_millis(40);

/// How long `bytes` take on the line at `baud` (11 bits each: start, 8 data, parity, stop).
fn transfer(bytes: usize, baud: Baud) -> Duration {
    let bits = u64::try_from(bytes.saturating_mul(11)).unwrap_or(u64::MAX);
    Duration::from_millis(bits.saturating_mul(1000) / u64::from(baud.bits_per_second()).max(1))
}

/// The most noise bytes in front of a converter's echo of the request.
const ECHO_NOISE: usize = 4;

/// The most bytes kept for one reply: more is noise, and the read ends.
const MAX_REPLY: usize = 512;

/// When the first byte must have arrived, and when a reply ends at the latest.
#[derive(Clone, Copy)]
struct Window {
    first: Instant,
    hard: Instant,
}

impl<P: Read + Write> Bus<P> {
    /// A bus at `baud` (the baud rate the port is opened with), with 50 ms of slack on
    /// every response timeout for USB and network latency.
    pub fn new(port: P, baud: Baud) -> Self {
        Self { port, baud, slack: Duration::from_millis(50), rx: Vec::with_capacity(512) }
    }

    /// Extra time added to every response timeout: more for a gateway on the network,
    /// zero for a UART on the same board.
    #[must_use]
    pub const fn slack(mut self, slack: Duration) -> Self {
        self.slack = slack;
        self
    }

    /// The port, e.g. to change its baud rate after [`master::set_baud`](crate::master::set_baud).
    pub fn port_mut(&mut self) -> &mut P {
        &mut self.port
    }

    /// The port back.
    pub fn into_inner(self) -> P {
        self.port
    }

    /// Read every telegram of one meter. Anything but a complete readout is an error
    /// ([`Error::Incomplete`]).
    pub fn readout(&mut self, target: Target) -> Result<Vec<Telegram>, Error> {
        self.run_readout(Readout::new(target))
    }

    /// Read every telegram with a configured [`Readout`] (retries, telegram limit,
    /// expected meter). `Ok` only when the readout is [complete](Outcome::Complete).
    pub fn run_readout(&mut self, mut readout: Readout) -> Result<Vec<Telegram>, Error> {
        let mut telegrams = Vec::new();
        while let Some(request) = readout.next_request() {
            self.send(&request)?;
            if !request.expects_reply() {
                readout.timeout(&[]);
                continue;
            }
            let deadline = self.deadline(&request);
            let mut start = 0;
            loop {
                let more = self.receive(deadline, request.bytes(), &mut start)?;
                let reply = self.rx.get(start..).unwrap_or(&[]);
                // Skip line noise in front of the frame (wired::scan); wait for the rest of
                // a frame that has started.
                let scan = if more { crate::wired::scan(reply) } else { crate::wired::scan_ended(reply) };
                let frame = match scan {
                    crate::wired::Scan::Frame { skip, len, .. } => reply.get(skip..skip + len),
                    crate::wired::Scan::NeedMore { .. } if more => continue,
                    crate::wired::Scan::NeedMore { .. } => None,
                };
                let event = match frame {
                    Some(frame) => readout.reply(frame),
                    None => readout.timeout(reply),
                };
                if let Event::Telegram(_) = event {
                    telegrams.push(Telegram { frame: frame.unwrap_or_default().to_vec() });
                }
                break;
            }
        }
        // The state machine ends every readout (after deselecting a secondary target).
        match readout.outcome() {
            Some(Outcome::Complete) => Ok(telegrams),
            outcome => Err(Error::Incomplete { outcome: outcome.unwrap_or(Outcome::NoReply), telegrams }),
        }
    }

    /// Find every meter on the bus by secondary address (wildcard search). Each
    /// selection waits for the whole response window, so colliding replies are seen.
    pub fn search(&mut self) -> Result<Found, Error> {
        self.run_search(Search::new())
    }

    /// Search with a configured [`Search`] (selection limit).
    pub fn run_search(&mut self, mut search: Search) -> Result<Found, Error> {
        let mut found = Found::default();
        while let Some(request) = search.next_request() {
            self.send(&request)?;
            let deadline = self.deadline(&request);
            let mut start = 0;
            // Collect until the line is quiet: a valid frame may be the first of several.
            while self.receive(deadline, request.bytes(), &mut start)? {}
            let reply = self.rx.get(start..).unwrap_or(&[]);
            let event = if reply.is_empty() { search.timeout(reply) } else { search.reply(reply) };
            match event {
                Event::Found(meter) => found.meters.push(meter),
                Event::Ambiguous(id) => found.ambiguous.push(id),
                Event::Unreadable(id) => found.unreadable.push(id),
                // A truncated frame at the end of the window: as good as garbage.
                Event::NeedMore => {
                    search.timeout(reply);
                }
                _ => {}
            }
        }
        found.complete = search.is_complete();
        Ok(found)
    }

    /// Send a command and wait for its acknowledgement: `Ok(true)` on `E5`, `Ok(false)`
    /// when nothing (or something else) came back. A broadcast is `Ok(true)` unread.
    pub fn command(&mut self, request: &Request) -> Result<bool, Error> {
        self.send(request)?;
        if !request.expects_reply() {
            return Ok(true);
        }
        let deadline = self.deadline(request);
        let mut start = 0;
        let ack =
            |rx: &[u8]| matches!(crate::wired::scan(rx), crate::wired::Scan::Frame { frame: Frame::Ack, .. });
        while self.receive(deadline, request.bytes(), &mut start)? {
            if ack(self.rx.get(start..).unwrap_or(&[])) {
                return Ok(true);
            }
        }
        Ok(matches!(
            crate::wired::scan_ended(self.rx.get(start..).unwrap_or(&[])),
            crate::wired::Scan::Frame { frame: Frame::Ack, .. }
        ))
    }

    fn deadline(&self, request: &Request) -> Window {
        // `flush` returns before the request is on the line (a USB converter, a socket):
        // the response timeout runs from its last bit.
        let first = Instant::now()
            + transfer(request.bytes().len(), self.baud)
            + Duration::from_millis(u64::from(request.timeout_ms(self.baud)))
            + self.slack;
        // The longest reply worth waiting for: echo, a 261-byte frame and some noise, then
        // a quiet line. A port that never falls silent ends here.
        Window { first, hard: first + transfer(MAX_REPLY, self.baud) + self.idle() + self.slack }
    }

    /// The quiet time that ends a reply: [`IDLE`] plus three character times.
    fn idle(&self) -> Duration {
        IDLE + transfer(3, self.baud)
    }

    fn send(&mut self, request: &Request) -> io::Result<()> {
        self.rx.clear();
        self.port.write_all(request.bytes())?;
        self.port.flush()
    }

    /// Read what arrives until `window.first` (for the first byte), until the line has been
    /// quiet for [`IDLE`], or at the latest until `window.hard` or [`MAX_REPLY`] bytes. `true` when new bytes arrived (call again for more), `false`
    /// when the reply is over. `start` skips an echo of `sent`.
    fn receive(&mut self, window: Window, sent: &[u8], start: &mut usize) -> io::Result<bool> {
        let mut chunk = [0u8; 256];
        let idle = self.idle();
        let mut until = if self.rx.len() > *start { Instant::now() + idle } else { window.first };
        loop {
            if self.rx.len() >= MAX_REPLY || Instant::now() >= window.hard {
                return Ok(false);
            }
            let n = match self.port.read(&mut chunk) {
                Ok(n) => n,
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
                    ) =>
                {
                    0
                }
                Err(e) => return Err(e),
            };
            if let Some(bytes) = chunk.get(..n).filter(|b| !b.is_empty()) {
                self.rx.extend_from_slice(bytes);
                // A converter that echoes: the request comes back first, perhaps after a
                // byte or two of noise from switching the line.
                if *start == 0 {
                    if let Some(at) = self.rx.windows(sent.len()).take(ECHO_NOISE + 1).position(|w| w == sent)
                    {
                        *start = at + sent.len();
                        until = window.first.max(Instant::now() + idle);
                    }
                }
                if self.rx.len() > *start {
                    return Ok(true);
                }
                continue;
            }
            if Instant::now() >= until {
                return Ok(false);
            }
            if n == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
