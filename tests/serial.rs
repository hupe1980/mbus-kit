//! The blocking serial driver (spec 005 US1, US4) against the simulated bus behind a
//! simulated converter: echo, line noise, replies split into small chunks, reads that
//! time out, and port errors.

mod sim;

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use mbus_kit::master::{Baud, Event, Readout, Search, SecondaryAddress, Target};
use mbus_kit::serial::{Bus, Error};

/// A level converter in front of the simulated bus.
struct Converter {
    bus: sim::Bus,
    echo: bool,
    noise: Vec<u8>,
    chunk: usize,
    rx: VecDeque<u8>,
    fail: bool,
}

impl Converter {
    fn new(ids: &[u32]) -> Self {
        Self {
            bus: sim::Bus::new(ids),
            echo: false,
            noise: Vec::new(),
            chunk: 64,
            rx: VecDeque::new(),
            fail: false,
        }
    }
}

impl Write for Converter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.fail {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "unplugged"));
        }
        if self.echo {
            self.rx.extend(buf);
        }
        let reply = self.bus.send(buf);
        if !reply.is_empty() {
            self.rx.extend(&self.noise);
        }
        self.rx.extend(reply);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Converter {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.rx.is_empty() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "nothing"));
        }
        let n = buf.len().min(self.chunk).min(self.rx.len());
        for b in buf.iter_mut().take(n) {
            *b = self.rx.pop_front().unwrap();
        }
        Ok(n)
    }
}

/// The telegrams the state machine yields when fed directly (no port).
fn direct(ids: &[u32], telegrams: u8, target: Target) -> Vec<Vec<u8>> {
    let mut bus = sim::Bus::new(ids);
    bus.meters.iter_mut().for_each(|m| m.telegrams = telegrams);
    let mut readout = Readout::new(target);
    let mut out = Vec::new();
    while let Some(req) = readout.next_request() {
        let reply = bus.send(req.bytes());
        let event = if reply.is_empty() { readout.timeout(&[]) } else { readout.reply(&reply) };
        if let Event::Telegram(_) = event {
            out.push(reply);
        }
    }
    out
}

fn fast(c: Converter) -> Bus<Converter> {
    Bus::new(c, Baud::B38400).slack(Duration::ZERO)
}

#[test]
fn readout_through_echo_noise_and_chunks_equals_the_state_machine() {
    // SC-002 (005)
    let want = direct(&[12_345_678], 3, Target::Primary(1));
    assert_eq!(want.len(), 3);
    for (echo, noise, chunk) in
        [(false, vec![], 64), (true, vec![], 64), (true, vec![0x00, 0xFF], 1), (false, vec![0x16], 3)]
    {
        let mut c = Converter::new(&[12_345_678]);
        c.bus.meters[0].telegrams = 3;
        (c.echo, c.noise, c.chunk) = (echo, noise.clone(), chunk);
        let got = fast(c).readout(Target::Primary(1)).unwrap();
        let got: Vec<Vec<u8>> = got.iter().map(|t| t.bytes().to_vec()).collect();
        assert_eq!(got, want, "echo {echo}, noise {noise:02X?}, chunk {chunk}");
    }
}

#[test]
fn telegrams_decode_and_secondary_readout_selects() {
    let mut c = Converter::new(&[12_345_678, 87_654_321]);
    c.echo = true;
    let mut bus = fast(c);
    let target = Target::Secondary(SecondaryAddress::id(87_654_321).unwrap());
    let telegrams = bus.readout(target).unwrap();
    let app = telegrams[0].application().unwrap();
    assert_eq!(app.meter().unwrap().to_string(), "KAM 87654321 heat (outlet) v1");
    assert_eq!(app.records().unwrap().count(), 1);
}

#[test]
fn a_silent_meter_is_no_reply_after_the_retries_and_bounded_in_time() {
    let started = Instant::now();
    let mut bus = fast(Converter::new(&[]));
    assert!(matches!(bus.readout(Target::Primary(7)), Err(Error::NoReply)));
    // SND_NKE three times at 38400 Bd: 3 × (9 ms + 50 ms), plus scheduling.
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(bus.into_inner().bus.log.len(), 3);
}

#[test]
fn port_errors_are_returned() {
    let mut c = Converter::new(&[12_345_678]);
    c.fail = true;
    let err = fast(c).readout(Target::Primary(1)).unwrap_err();
    assert!(matches!(&err, Error::Io(e) if e.kind() == io::ErrorKind::BrokenPipe), "{err}");
    assert_eq!(err.to_string(), "port: unplugged");
}

#[test]
fn search_finds_every_meter_through_the_converter() {
    // US4: two meters collide on the first digit, echo on. (Every empty probe waits the
    // full response timeout, as on a real bus: the collision depth sets the test time.)
    let ids = [12_345_678, 19_999_999, 87_654_321];
    let mut c = Converter::new(&ids);
    c.echo = true;
    let found = fast(c).search().unwrap();
    let mut got: Vec<String> = found.meters.iter().map(|a| a.id_number().unwrap().to_string()).collect();
    got.sort();
    assert_eq!(got, ["12345678", "19999999", "87654321"]);
    assert!(found.ambiguous.is_empty());
    assert!(found.complete);
}

#[test]
fn commands_are_acknowledged() {
    let mut bus = fast(Converter::new(&[12_345_678]));
    assert!(bus.command(&mbus_kit::master::link_reset(1)).unwrap());
    assert!(!bus.command(&mbus_kit::master::link_reset(9)).unwrap(), "nobody at 9");
    assert!(bus.command(&mbus_kit::master::link_reset(0xFF)).unwrap(), "broadcast is not answered");
}

#[test]
fn a_tcp_gateway_is_a_port_too() {
    // A real socket with a read timeout: the driver's timing over std::net, as against an
    // Ethernet M-Bus gateway.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let gateway = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bus = sim::Bus::new(&[12_345_678]);
        bus.meters[0].telegrams = 2;
        let mut buf = [0u8; 64];
        let mut pending = Vec::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            if let Ok(Some((_, len))) = mbus_kit::wired::Frame::parse(&pending) {
                let reply = bus.send(&pending[..len]);
                pending.drain(..len);
                // Deliver in two pieces, 5 ms apart.
                let (a, b) = reply.split_at(reply.len() / 2);
                stream.write_all(a).unwrap();
                std::thread::sleep(Duration::from_millis(5));
                stream.write_all(b).unwrap();
            }
        }
    });
    let stream = std::net::TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    let mut bus = Bus::new(stream, Baud::B9600);
    let telegrams = bus.readout(Target::Primary(1)).unwrap();
    assert_eq!(telegrams.len(), 2);
    drop(bus);
    gateway.join().unwrap();
}

/// A converter that answers every request with random bytes (sometimes a valid E5).
struct Noise {
    x: u64,
    rx: VecDeque<u8>,
}

impl Write for Noise {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.x ^= self.x << 13;
        self.x ^= self.x >> 7;
        self.x ^= self.x << 17;
        let n = (self.x % 40) as usize;
        if self.x % 5 == 0 {
            self.rx.push_back(0xE5);
        }
        for i in 0..n {
            self.rx.push_back((self.x >> (i % 56)) as u8);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Noise {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.rx.len()).min(7);
        for b in buf.iter_mut().take(n) {
            *b = self.rx.pop_front().unwrap();
        }
        if n == 0 { Err(io::ErrorKind::WouldBlock.into()) } else { Ok(n) }
    }
}

#[test]
fn garbage_from_the_bus_ends_every_readout() {
    // 005-io-shells/SR-001: bounded, no panic, whatever arrives.
    for seed in 1..=20u64 {
        let port = Noise { x: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15), rx: VecDeque::new() };
        let mut bus = Bus::new(port, Baud::B38400).slack(Duration::ZERO);
        let started = Instant::now();
        let _ = bus.readout(Target::Primary(1));
        assert!(started.elapsed() < Duration::from_secs(5), "seed {seed}");
    }
}

/// A port that never stops sending.
struct Babbler(u8);

impl Write for Babbler {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Babbler {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0 = self.0.wrapping_add(1);
        buf.fill(self.0 | 0x01); // never a valid frame start alone, never quiet
        Ok(buf.len())
    }
}

#[test]
fn a_port_that_never_falls_silent_cannot_hold_the_driver() {
    // 005-io-shells/SR-001: time and memory are bounded per reply.
    let started = Instant::now();
    let mut bus = Bus::new(Babbler(0), Baud::B9600).slack(Duration::ZERO);
    assert!(matches!(bus.readout(Target::Primary(1)), Err(Error::NoReply)));
    // Every probe looks like a collision: the search gives up at its limit and says so.
    let found = bus.run_search(Search::new().max_selections(200)).unwrap();
    assert!(!found.complete);
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
}

/// A meter that acknowledges SND_NKE and answers REQ_UD2 with one long frame whose data
/// contains `E5`, delivered in 24-byte chunks like a USB converter.
struct E5InData {
    rx: VecDeque<u8>,
}

impl Write for E5InData {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf == [0x10, 0x40, 0x05, 0x45, 0x16] {
            self.rx.push_back(0xE5);
        } else {
            let data =
                [0x78, 0x56, 0x34, 0x12, 0x24, 0x40, 0x01, 0x07, 0x55, 0, 0, 0, 0x03, 0x13, 0xE5, 0x31, 0x00];
            let mut out = [0u8; 64];
            let n = mbus_kit::wired::Frame::Long { c: 0x08, a: 5, ci: 0x72, data: &data }
                .encode(&mut out)
                .unwrap();
            self.rx.extend(&out[..n]);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for E5InData {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.rx.len()).min(24);
        if n == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        for b in buf.iter_mut().take(n) {
            *b = self.rx.pop_front().unwrap();
        }
        Ok(n)
    }
}

#[test]
fn an_e5_inside_a_frame_still_arriving_is_data_not_an_ack() {
    // Independent review 2026-11-27: the readout used to end "complete" with no telegram.
    let mut bus = Bus::new(E5InData { rx: VecDeque::new() }, Baud::B9600).slack(Duration::ZERO);
    let telegrams = bus.readout(Target::Primary(5)).unwrap();
    assert_eq!(telegrams.len(), 1);
    assert_eq!(telegrams[0].application().unwrap().records().unwrap().count(), 1);
}
