//! AFL fragment reassembly (OMS Vol. 2 §6.2, §7.3; EN 13757-7 clause 6).
//!
//! A message too long for one frame is sent in fragments, each in its own frame with an
//! AFL (CI `90`) in front. The fragment IDs count 1, 2, 3, …; *more fragments* is set
//! on all but the last. The first fragment carries the message control, counter and
//! length, and the transport layer starts in it; the last carries the MAC over the
//! whole message. Fragments are requested one by one (`REQ_UD2` or `SND_UD`), never
//! broadcast, so they arrive in order.
//!
//! ```text
//! fragment 1:  … 90 AFLL FCL(MF, ID 1) MCL MCR ML | 7A header …
//! fragment 2:  … 90 AFLL FCL(MF, ID 2)            | … middle …
//! fragment 3:  … 90 AFLL FCL(ID 3) MAC            | … end
//! ```
//!
//! [`Reassembly`] copies the pieces into a buffer the caller owns, checks the order,
//! the sender and the announced length, and parses the whole message, which then
//! decrypts and authenticates like an unfragmented one. One `Reassembly` per meter.
//!
//! ```
//! use mbus_kit::fragment::Reassembly;
//! use mbus_kit::{Application, Body};
//!
//! // A 3-byte message (CI 78 and two record bytes) in two fragments.
//! # let f1 = [0x90, 0x05, 0x01, 0x70, 0x40, 0x03, 0x00, 0x78, 0xAA];
//! # let f2 = [0x90, 0x02, 0x02, 0x00, 0xBB];
//! let mut buf = [0u8; 256];
//! let mut message = Reassembly::new(&mut buf);
//! assert!(message.push(&Application::parse(f1[0], &f1[1..])?)?.is_none()); // more to come
//! let app = message.push(&Application::parse(f2[0], &f2[1..])?)?.expect("last fragment");
//! assert!(matches!(app.body, Body::Records(_)));
//! # Ok::<(), mbus_kit::Error>(())
//! ```

use crate::app::{Address, Afl, Application, Authenticity, Body, Ell};
use crate::{Error, Result};

/// Joins the fragments of one message into a caller buffer.
#[derive(Debug)]
pub struct Reassembly<'b> {
    buf: &'b mut [u8],
    len: usize,
    /// The fragment ID expected next; 0 while no message is open.
    next: u8,
    first: Option<Afl>,
    sender: Option<Address>,
    ell: Option<Ell>,
    decrypted: bool,
}

impl<'b> Reassembly<'b> {
    /// Reassemble into `buf`. OMS limits a message to 16 KiB; size `buf` for the longest
    /// message the meters send.
    pub const fn new(buf: &'b mut [u8]) -> Self {
        Self { buf, len: 0, next: 0, first: None, sender: None, ell: None, decrypted: false }
    }

    /// The fragment ID expected next, or `None` while no message is open (the next one
    /// must be fragment 1).
    pub const fn expected(&self) -> Option<u8> {
        if self.next == 0 { None } else { Some(self.next) }
    }

    /// The bytes collected so far.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing has been collected.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Drop a message in progress.
    pub fn reset(&mut self) {
        self.len = 0;
        self.next = 0;
        self.first = None;
        self.sender = None;
        self.ell = None;
        self.decrypted = false;
    }

    /// Add a fragment. `Ok(None)` while more are expected; the whole message, parsed,
    /// after the last one.
    ///
    /// Fragment 1 always starts a new message (a meter or gateway that starts over, OMS
    /// Vol. 2 Annex L.4). Any other fragment must be the expected one from the same
    /// meter: otherwise [`Error::UnexpectedFragment`] (missing, repeated, out of order,
    /// or not a fragment at all: `found` 0) or [`Error::FragmentFromOtherMeter`], and the
    /// message in progress is kept, so the caller can request the fragment again. A
    /// message longer than the buffer is [`Error::OutputTooSmall`], one whose length
    /// differs from the length announced in fragment 1 [`Error::InvalidLength`]; both
    /// drop the message.
    pub fn push(&mut self, fragment: &Application<'_>) -> Result<Option<Application<'_>>> {
        let (Body::Fragment(data), Some(afl)) = (&fragment.body, fragment.afl) else {
            return Err(Error::UnexpectedFragment { expected: self.next.max(1), found: 0 });
        };
        let id = afl.fragment_id();
        if id == 1 {
            self.reset();
            if afl.length.is_some_and(|ml| usize::from(ml) > self.buf.len()) {
                return Err(Error::OutputTooSmall);
            }
            self.first = Some(afl);
            self.sender = fragment.meter();
            self.ell = fragment.ell;
            self.decrypted = fragment.authenticity == Authenticity::Encrypted;
        } else if self.next == 0 || id != self.next {
            return Err(Error::UnexpectedFragment { expected: self.next.max(1), found: id });
        } else if fragment.meter() != self.sender {
            return Err(Error::FragmentFromOtherMeter);
        } else {
            self.decrypted &= fragment.authenticity == Authenticity::Encrypted;
        }
        let Some(dst) = self.buf.get_mut(self.len..self.len + data.len()) else {
            self.reset();
            return Err(Error::OutputTooSmall);
        };
        dst.copy_from_slice(data);
        self.len += data.len();
        if afl.more_fragments() {
            // The fragment ID never rolls over: 255 fragments at most.
            let Some(next) = id.checked_add(1) else {
                self.reset();
                return Err(Error::UnexpectedFragment { expected: 0, found: id });
            };
            self.next = next;
            return Ok(None);
        }
        let first = self.first.unwrap_or(afl);
        let (len, ell, sender, decrypted) = (self.len, self.ell, self.sender, self.decrypted);
        self.reset();
        if first.length.is_some_and(|ml| usize::from(ml) != len) {
            return Err(Error::InvalidLength);
        }
        // The last fragment's MAC has the length the first fragment's MCL announced.
        if !afl.mac().is_empty() && first.mcl.and_then(crate::app::mac_len) != Some(afl.mac().len()) {
            return Err(Error::InvalidLength);
        }
        let message = self.buf.get(..len).ok_or(Error::OutputTooSmall)?;
        Application::reassembled(message, Afl::joined(&first, &afl), ell, sender, decrypted).map(Some)
    }
}
