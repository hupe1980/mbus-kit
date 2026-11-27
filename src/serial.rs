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
use crate::master::{Baud, Event, Readout, Request, Search, SecondaryAddress, Target};
use crate::wired::Frame;

/// What can go wrong on a real bus.
#[derive(Debug)]
pub enum Error {
    /// The port failed.
    Io(io::Error),
    /// The meter did not answer, after every retry.
    NoReply,
    /// The meter answered with a frame that is not a valid telegram.
    Invalid(crate::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "port: {e}"),
            Self::NoReply => f.write_str("the meter does not answer"),
            Self::Invalid(e) => write!(f, "invalid reply: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Invalid(e) => Some(e),
            Self::NoReply => None,
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

    /// Read every telegram of one meter.
    pub fn readout(&mut self, target: Target) -> Result<Vec<Telegram>, Error> {
        self.run_readout(Readout::new(target))
    }

    /// Read every telegram with a configured [`Readout`] (retries, telegram limit).
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
                let frame = match crate::wired::scan(reply) {
                    crate::wired::Scan::Frame { skip, len, .. } => reply.get(skip..skip + len),
                    crate::wired::Scan::NeedMore { .. } if more => continue,
                    crate::wired::Scan::NeedMore { .. } => None,
                };
                let event = match frame {
                    Some(frame) => readout.reply(frame),
                    None => readout.timeout(reply),
                };
                match event {
                    Event::Telegram(_) => {
                        telegrams.push(Telegram { frame: frame.unwrap_or_default().to_vec() });
                    }
                    Event::Invalid(e) => return Err(Error::Invalid(e)),
                    _ => {}
                }
                break;
            }
        }
        if readout.is_complete() { Ok(telegrams) } else { Err(Error::NoReply) }
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
        while self.receive(deadline, request.bytes(), &mut start)? {
            if self.rx.get(start..) == Some(&[0xE5][..]) {
                return Ok(true);
            }
        }
        Ok(self.rx.get(start..) == Some(&[0xE5][..]))
    }

    fn deadline(&self, request: &Request) -> Window {
        let first =
            Instant::now() + Duration::from_millis(u64::from(request.timeout_ms(self.baud))) + self.slack;
        // The longest reply worth waiting for: echo, a 261-byte frame and some noise, at
        // 11 bits per byte, then a quiet line. A port that never falls silent ends here.
        let bits = u64::try_from(MAX_REPLY * 11).unwrap_or(u64::MAX);
        let transfer = Duration::from_millis(bits * 1000 / u64::from(self.baud.bits_per_second()).max(1));
        Window { first, hard: first + transfer + IDLE + self.slack }
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
        let mut until = if self.rx.len() > *start { Instant::now() + IDLE } else { window.first };
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
                // A converter that echoes: the request comes back first.
                if *start == 0 && self.rx.len() >= sent.len() && self.rx.starts_with(sent) {
                    *start = sent.len();
                    until = window.first.max(Instant::now() + IDLE);
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
