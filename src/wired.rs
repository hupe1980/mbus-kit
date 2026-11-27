//! Wired M-Bus link layer (EN 13757-2, the FT1.2 format of IEC 60870-5-2).
//!
//! ```text
//! E5                                      single character (ACK)
//! 10 C A CS 16                            short frame
//! 68 L L 68 C A CI [data…] CS 16          control (L = 3) and long frame
//! ```
//!
//! `L` counts the bytes from `C` to the last data byte. `CS` is their arithmetic sum
//! modulo 256 (for a short frame, `C + A`).

use crate::{Error, Result};

/// The single-character acknowledgement.
pub const ACK: u8 = 0xE5;
const SHORT_START: u8 = 0x10;
const LONG_START: u8 = 0x68;
const STOP: u8 = 0x16;

/// C-field values a master sends.
pub mod control {
    /// `SND_NKE`: initialise the slave (reset the FCB).
    pub const SND_NKE: u8 = 0x40;
    /// `SND_UD`: send user data to the slave (FCB clear).
    pub const SND_UD: u8 = 0x53;
    /// `REQ_UD2`: request class 2 data, i.e. the meter reading (FCB clear).
    pub const REQ_UD2: u8 = 0x5B;
    /// The frame count bit, toggled between successive requests.
    pub const FCB: u8 = 0x20;
    /// `RSP_UD`: a slave's response (lower nibble 8; ACD and DFC bits may be set).
    pub const RSP_UD: u8 = 0x08;

    /// Whether `c` is a slave's response with data: `RSP_UD`, PRM clear, ACD and DFC
    /// either way (`08`, `18`, `28`, `38`).
    pub const fn is_response(c: u8) -> bool {
        c & 0xCF == RSP_UD
    }
}

/// The primary address that addresses every slave (no reply expected).
pub const BROADCAST: u8 = 0xFF;
/// The primary address that addresses the selected slave (secondary addressing).
pub const NETWORK_LAYER: u8 = 0xFD;

/// A wired M-Bus frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame<'a> {
    /// `E5`: acknowledgement.
    Ack,
    /// A short frame: a request without data.
    Short {
        /// C field.
        c: u8,
        /// Primary address.
        a: u8,
    },
    /// A long (or control, when `data` is empty) frame.
    Long {
        /// C field.
        c: u8,
        /// Primary address.
        a: u8,
        /// CI field: what the data is.
        ci: u8,
        /// The application data after the CI field.
        data: &'a [u8],
    },
}

fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |s, &b| s.wrapping_add(b))
}

impl<'a> Frame<'a> {
    /// Parse one frame at the start of `buf`.
    ///
    /// Returns `Ok(None)` when `buf` holds the beginning of a valid frame but not all of
    /// it yet (read more and call again), and the frame with the number of bytes it
    /// used otherwise.
    pub fn parse(buf: &'a [u8]) -> Result<Option<(Self, usize)>> {
        let Some(&start) = buf.first() else { return Ok(None) };
        match start {
            ACK => Ok(Some((Self::Ack, 1))),
            SHORT_START => {
                let Some(&[_, c, a, cs, stop]) = buf.get(..5).and_then(|s| <&[u8; 5]>::try_from(s).ok())
                else {
                    return Ok(None);
                };
                check(sum(&[c, a]), cs, stop)?;
                Ok(Some((Self::Short { c, a }, 5)))
            }
            LONG_START => {
                let Some(&[_, l1, l2, s2]) = buf.get(..4).and_then(|s| <&[u8; 4]>::try_from(s).ok()) else {
                    return Ok(None);
                };
                if l1 != l2 || s2 != LONG_START || l1 < 3 {
                    return Err(Error::InvalidHeader);
                }
                let l = usize::from(l1);
                let total = l + 6;
                let Some(frame) = buf.get(..total) else { return Ok(None) };
                let user = frame.get(4..4 + l).ok_or(Error::UnexpectedEnd)?;
                let cs = frame.get(4 + l).copied().ok_or(Error::UnexpectedEnd)?;
                let stop = frame.get(5 + l).copied().ok_or(Error::UnexpectedEnd)?;
                check(sum(user), cs, stop)?;
                let (&c, rest) = user.split_first().ok_or(Error::UnexpectedEnd)?;
                let (&a, rest) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
                let (&ci, data) = rest.split_first().ok_or(Error::UnexpectedEnd)?;
                Ok(Some((Self::Long { c, a, ci, data }, total)))
            }
            other => Err(Error::InvalidStart(other)),
        }
    }

    /// Write the frame into `out`. Returns the number of bytes written.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize> {
        match *self {
            Self::Ack => {
                *out.first_mut().ok_or(Error::OutputTooSmall)? = ACK;
                Ok(1)
            }
            Self::Short { c, a } => {
                let bytes = [SHORT_START, c, a, sum(&[c, a]), STOP];
                out.get_mut(..5).ok_or(Error::OutputTooSmall)?.copy_from_slice(&bytes);
                Ok(5)
            }
            Self::Long { c, a, ci, data } => {
                let l = data.len().checked_add(3).filter(|&l| l <= 255).ok_or(Error::InvalidLength)?;
                let total = l + 6;
                let frame = out.get_mut(..total).ok_or(Error::OutputTooSmall)?;
                let l8 = u8::try_from(l).map_err(|_| Error::InvalidLength)?;
                let (head, rest) = frame.split_at_mut(7);
                head.copy_from_slice(&[LONG_START, l8, l8, LONG_START, c, a, ci]);
                let (body, tail) = rest.split_at_mut(data.len());
                body.copy_from_slice(data);
                let cs = sum(&[c, a, ci]).wrapping_add(sum(data));
                tail.copy_from_slice(&[cs, STOP]);
                Ok(total)
            }
        }
    }
}

/// What [`scan`] found in a receive buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scan<'a> {
    /// A valid frame starting `skip` bytes into the buffer and using `len` bytes. Drop
    /// `skip + len` bytes before scanning again.
    Frame {
        /// The frame.
        frame: Frame<'a>,
        /// Bytes of noise before it.
        skip: usize,
        /// Bytes the frame uses.
        len: usize,
    },
    /// No complete frame yet: drop `skip` bytes of noise, keep the rest, read more.
    NeedMore {
        /// Bytes that cannot start a frame and can be dropped.
        skip: usize,
    },
}

/// Find the first valid frame in a receive buffer that may begin with noise, an echo or
/// the tail of a broken frame (what a level converter delivers on a real bus).
///
/// Every byte that cannot start a valid frame is skipped. The first start byte whose frame
/// is still incomplete wins: the rest may be arriving, and its data can contain bytes that
/// look like a frame of their own (an `E5`, a `10 … 16`). A stray start byte in front of a
/// reply is resolved as soon as enough bytes arrive to show its frame is invalid (wrong
/// length bytes, checksum or stop byte). Checksums are verified for every frame.
pub fn scan(buf: &[u8]) -> Scan<'_> {
    for start in 0..buf.len() {
        let Some(rest) = buf.get(start..) else { break };
        match Frame::parse(rest) {
            Ok(Some((frame, len))) => return Scan::Frame { frame, skip: start, len },
            Ok(None) => return Scan::NeedMore { skip: start },
            Err(_) => {}
        }
    }
    Scan::NeedMore { skip: buf.len() }
}

/// [`scan`] for a reply that is over (no more bytes will come): an incomplete frame is
/// noise too, so a frame behind a stray start byte (`68 E5`, `10 E5`) is still found.
/// [`Scan::NeedMore`] means there is no valid frame.
pub fn scan_ended(buf: &[u8]) -> Scan<'_> {
    for start in 0..buf.len() {
        if let Some(Ok(Some((frame, len)))) = buf.get(start..).map(Frame::parse) {
            return Scan::Frame { frame, skip: start, len };
        }
    }
    Scan::NeedMore { skip: buf.len() }
}

fn check(computed: u8, found: u8, stop: u8) -> Result<()> {
    if computed != found {
        return Err(Error::Checksum { computed, found });
    }
    if stop != STOP {
        return Err(Error::MissingStop);
    }
    Ok(())
}

/// `SND_NKE` to a primary address: reset the slave's link layer.
pub const fn snd_nke(address: u8) -> Frame<'static> {
    Frame::Short { c: control::SND_NKE, a: address }
}

/// `REQ_UD2` to a primary address: ask for the meter reading. Toggle `fcb` between
/// successive requests to the same slave.
pub const fn req_ud2(address: u8, fcb: bool) -> Frame<'static> {
    Frame::Short { c: control::REQ_UD2 | if fcb { control::FCB } else { 0 }, a: address }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_frame_round_trip() {
        let mut out = [0u8; 8];
        let n = req_ud2(5, true).encode(&mut out).unwrap();
        assert_eq!(&out[..n], &[0x10, 0x7B, 0x05, 0x80, 0x16]);
        assert_eq!(Frame::parse(&out[..n]).unwrap(), Some((req_ud2(5, true), 5)));
    }

    #[test]
    fn incomplete_frames_ask_for_more() {
        let mut out = [0u8; 16];
        let n = Frame::Long { c: 0x08, a: 1, ci: 0x72, data: &[1, 2, 3] }.encode(&mut out).unwrap();
        for cut in 0..n {
            assert_eq!(Frame::parse(&out[..cut]).unwrap(), None, "cut at {cut}");
        }
        assert!(Frame::parse(&out[..n]).unwrap().is_some());
    }

    #[test]
    fn scan_skips_noise_and_false_starts() {
        let mut buf = [0u8; 32];
        let n = req_ud2(5, true).encode(&mut buf[3..]).unwrap();
        buf[..3].copy_from_slice(&[0x00, 0x68, 0x42]); // noise, a false long-frame start
        let all = &buf[..3 + n];
        assert_eq!(scan(all), Scan::Frame { frame: req_ud2(5, true), skip: 3, len: 5 });
        // Incomplete: keep from the first plausible start (68 42 10 7B is no header).
        assert_eq!(scan(&all[..3 + 3]), Scan::NeedMore { skip: 3 });
        assert_eq!(scan(&[0x00, 0x68, 0x05]), Scan::NeedMore { skip: 1 });
        assert_eq!(scan(&[0x00, 0x01, 0x02]), Scan::NeedMore { skip: 3 });
        assert_eq!(scan(&[]), Scan::NeedMore { skip: 0 });
        // An ACK after an echo of our own request.
        let mut echo = [0u8; 6];
        echo[..5].copy_from_slice(&all[3..]);
        echo[5] = ACK;
        assert_eq!(scan(&echo), Scan::Frame { frame: req_ud2(5, true), skip: 0, len: 5 });
        // A long frame still arriving whose data holds an E5: wait, never an ACK.
        let mut long = [0u8; 64];
        let data =
            [0x78, 0x56, 0x34, 0x12, 0x24, 0x40, 0x01, 0x07, 0x55, 0, 0, 0, 0x03, 0x13, 0xE5, 0x31, 0x00];
        let n = Frame::Long { c: 0x08, a: 5, ci: 0x72, data: &data }.encode(&mut long).unwrap();
        for cut in 1..n {
            assert_eq!(scan(&long[..cut]), Scan::NeedMore { skip: 0 }, "cut {cut}");
        }
        assert!(matches!(scan(&long[..n]), Scan::Frame { skip: 0, .. }));
    }

    #[test]
    fn bad_checksum_and_stop() {
        assert!(matches!(Frame::parse(&[0x10, 0x5B, 0x01, 0x00, 0x16]), Err(Error::Checksum { .. })));
        assert_eq!(Frame::parse(&[0x10, 0x5B, 0x01, 0x5C, 0x17]), Err(Error::MissingStop));
        assert_eq!(Frame::parse(&[0x68, 0x05, 0x06, 0x68]), Err(Error::InvalidHeader));
        assert_eq!(Frame::parse(&[0x42]), Err(Error::InvalidStart(0x42)));
    }
}
