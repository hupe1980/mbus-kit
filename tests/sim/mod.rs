//! A simulated wired M-Bus shared by the master and serial tests: meters that answer
//! selections and reads like real ones, including collisions.
#![allow(dead_code)]

use mbus_kit::wired::Frame;
use mbus_kit::{Address, Manufacturer, Medium};

/// One simulated meter: its address and the telegrams it sends.
pub struct Meter {
    pub address: Address,
    pub primary: u8,
    pub selected: bool,
    pub telegrams: u8,
    pub sent: u8,
    pub last_fcb: Option<bool>,
}

pub struct Bus {
    pub meters: Vec<Meter>,
    pub log: Vec<Vec<u8>>,
}

pub fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

pub fn addr(id: u32) -> Address {
    let s = format!("{id:08}");
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap();
    Address {
        manufacturer: Manufacturer::from_letters(*b"KAM").unwrap(),
        id: [b(6), b(4), b(2), b(0)],
        version: 1,
        medium: Medium::HEAT,
    }
}

impl Bus {
    pub fn new(ids: &[u32]) -> Self {
        let meters = ids
            .iter()
            .enumerate()
            .map(|(i, &id)| Meter {
                address: addr(id),
                primary: u8::try_from(i + 1).unwrap(),
                selected: false,
                telegrams: 1,
                sent: 0,
                last_fcb: None,
            })
            .collect();
        Self { meters, log: Vec::new() }
    }

    /// The meter's reply to a REQ_UD2: a long-header telegram, `1F` until the last one.
    pub fn telegram(m: &Meter, primary: u8) -> Vec<u8> {
        let a = m.address;
        let [i0, i1, i2, i3] = a.id;
        let [m0, m1] = a.manufacturer.0.to_le_bytes();
        let mut data = vec![i0, i1, i2, i3, m0, m1, a.version, a.medium.0, m.sent, 0, 0, 0];
        data.extend_from_slice(&[0x04, 0x13, m.sent, 0, 0, 0]); // volume = telegram number
        if m.sent + 1 < m.telegrams {
            data.push(0x1F);
        }
        let mut out = [0u8; 64];
        let n = Frame::Long { c: 0x08, a: primary, ci: 0x72, data: &data }.encode(&mut out).unwrap();
        out[..n].to_vec()
    }

    /// What arrives on the bus after `request`.
    pub fn send(&mut self, request: &[u8]) -> Vec<u8> {
        self.log.push(request.to_vec());
        let (frame, _) = Frame::parse(request).unwrap().unwrap();
        let mut replies: Vec<Vec<u8>> = Vec::new();
        match frame {
            Frame::Short { c, a } if c & 0x4F == 0x40 => {
                // SND_NKE: to FD deselects; to a primary address resets.
                for m in &mut self.meters {
                    if a == 0xFD {
                        if m.selected {
                            replies.push(vec![0xE5]);
                        }
                        m.selected = false;
                    } else if m.primary == a {
                        m.last_fcb = None;
                        m.sent = 0;
                        replies.push(vec![0xE5]);
                    }
                }
            }
            Frame::Short { c, a } if c & 0x4F == 0x4B => {
                let fcb = c & 0x20 != 0;
                for m in &mut self.meters {
                    if (a == 0xFD && m.selected) || (a != 0xFD && m.primary == a) {
                        // A repeated FCB means "send the same again" (EN 13757-2).
                        if m.last_fcb == Some(fcb) && m.sent > 0 {
                            m.sent -= 1;
                        }
                        m.last_fcb = Some(fcb);
                        let pa = if a == 0xFD { 0xFD } else { m.primary };
                        replies.push(Self::telegram(m, pa));
                        m.sent = (m.sent + 1).min(m.telegrams);
                    }
                }
            }
            Frame::Long { a: 0xFD, ci: 0x52, data, .. } => {
                let pattern: [u8; 8] = data.try_into().unwrap();
                for m in &mut self.meters {
                    m.selected = matches(&pattern, &m.address);
                    if m.selected {
                        replies.push(vec![0xE5]);
                    }
                }
            }
            _ => {}
        }
        match replies.len() {
            0 => Vec::new(),
            1 => replies.pop().unwrap(),
            // Identical E5s overlap into one E5; anything else garbles.
            _ if replies.iter().all(|r| r == &[0xE5]) => vec![0xE5],
            _ => vec![0x68, 0x13, 0x37, 0x68, 0xFF, 0x00],
        }
    }
}

/// How a meter matches a selection (wildcard nibbles F, FFFF, FF).
pub fn matches(p: &[u8; 8], a: &Address) -> bool {
    let id_ok = p[..4].iter().zip(a.id).all(|(&want, got)| {
        let nib = |w: u8, g: u8| w == 0xF || w == g;
        nib(want >> 4, got >> 4) && nib(want & 0xF, got & 0xF)
    });
    let m = u16::from_le_bytes([p[4], p[5]]);
    id_ok
        && (m == 0xFFFF || m == a.manufacturer.0)
        && (p[6] == 0xFF || p[6] == a.version)
        && (p[7] == 0xFF || p[7] == a.medium.0)
}
