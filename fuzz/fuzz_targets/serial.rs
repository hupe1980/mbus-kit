//! Arbitrary replies through the blocking driver (005-io-shells/SR-001): the port replays
//! the input in chunks and then fails, so no read waits for a timeout; readout and search
//! must end without a panic whatever the "bus" sent.
#![no_main]

use std::io::{self, Read, Write};

use libfuzzer_sys::fuzz_target;
use mbus_kit::master::{Baud, Search, Target};
use mbus_kit::serial::Bus;

struct Replay<'a> {
    data: &'a [u8],
    chunk: usize,
}

impl Write for Replay<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Replay<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.data.is_empty() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let n = buf.len().min(self.chunk).min(self.data.len());
        buf[..n].copy_from_slice(&self.data[..n]);
        self.data = &self.data[n..];
        Ok(n)
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&size, data)) = data.split_first() else { return };
    let chunk = usize::from(size % 32) + 1;
    let mut bus = Bus::new(Replay { data, chunk }, Baud::B38400).slack(std::time::Duration::ZERO);
    if let Ok(telegrams) = bus.readout(Target::Primary(1)) {
        for t in telegrams {
            let _ = t.application();
        }
    }
    let mut bus = Bus::new(Replay { data, chunk }, Baud::B38400).slack(std::time::Duration::ZERO);
    let _ = bus.run_search(Search::new().max_selections(64));
});
