+++
title = "Wired master"
description = "Read every telegram of a meter, address meters by their printed ID, find all meters on a bus, and configure them: sans-I/O, with the frame count bit and retries handled."
weight = 15
+++

## The loop

The library says what to send and how long to wait; you move the bytes. Every state
machine has the same three calls:

```rust
use mbus_kit::master::{Baud, Event, Readout, Target};

let mut readout = Readout::new(Target::Primary(5));
while let Some(request) = readout.next_request() {
    port.write_all(request.bytes())?;
    let reply = port.read_frame(request.timeout_ms(Baud::B2400))?; // bytes, or empty on timeout
    let event = if reply.is_empty() { readout.timeout(&reply) } else { readout.reply(&reply) };
    if let Event::Telegram(app) = event {
        for record in app.records().into_iter().flatten() {
            println!("{}", record?);
        }
    }
}
```

`reply` answers `Event::NeedMore` while the bytes are the start of a frame: read on and
call it again with everything received. `wired::scan` finds a frame behind line noise or
an echo, if your converter produces them.

## Read a meter completely

`Readout` sends `SND_NKE`, then `REQ_UD2` until a telegram no longer ends with "more
records follow" (DIF `1F`). The frame count bit is handled as EN 13757-2 requires: 1
after the reset, toggled after every valid reply, unchanged on a repetition so the meter
sends the same telegram again. A request without a valid reply is repeated twice
(`.retries(n)` to change), then the readout reports `Event::NoReply`. `.max_telegrams(n)`
caps meters that always say more follows (default 16).

## Address a meter by its ID

```rust
use mbus_kit::master::{Readout, SecondaryAddress, Target};

let meter = SecondaryAddress::id(12_345_678).unwrap();          // manufacturer, version, medium: any
let mut readout = Readout::new(Target::Secondary(meter));       // deselect all, select, read, deselect
let some = SecondaryAddress::pattern("1234FFFF").unwrap();      // wildcards: F, * or ?
```

Two meters sharing an ID: `master::select_enhanced(&meter, Some(fabrication_number))`.

## Find every meter on the bus

```rust
use mbus_kit::master::{Event, Search};

let mut search = Search::new();
while let Some(request) = search.next_request() {
    // send, wait, then:
    match search.reply(&reply) /* or search.timeout(&partial) */ {
        Event::Found(meter) => println!("{meter}"),           // KAM 12345678 heat (outlet) v8
        Event::Ambiguous(id) => println!("two meters share {id}"),
        _ => {}
    }
}
```

The search selects with the first digit fixed (`0FFFFFFF` … `9FFFFFFF`). No answer: no
meter there. One `E5`: exactly one meter, which it reads to learn the full address.
Garbage, extra bytes, or a garbled read after an `E5` (identical `E5`s overlap into one
clean byte): several meters collided, so it fixes the next digit. Pass whatever arrived
before the timeout to `timeout(&partial)`: partial bytes are how a collision shows. The
search ends after at most 10 × 8 × (meters + 1) × 3 selections; a bus of 250 meters is in
the tests.

## Configure a meter

| Command | Function | Frame |
|---|---|---|
| Reset the link layer | `master::link_reset(5)` | `10 40 05 45 16` |
| Set the primary address | `master::set_primary_address(0, 17)` | `68 06 06 68 53 00 51 01 7A 11 …` |
| Reset the application | `master::application_reset(5, None)` | `68 03 03 68 53 05 50 …` |
| Change the baud rate | `master::set_baud(5, Baud::B9600)` | CI `BD`; the meter acknowledges at the old rate |
| Set the clock | `master::set_time(5, &DateTime::new(2026, 11, 27, 9, 36)?)` | `04 6D` + type F |
| Ask for all data | `master::request_all_data(5)` | `7F` |

Before setting a primary address, check that nothing answers at the new one.

## Timing

A reply must start within 330 bit times plus 50 ms (EN 13757-2):
`request.timeout_ms(baud)` gives 188 ms at 2400 Bd, 85 ms at 9600 Bd (the 50 ms do not
scale with the baud rate). The sans-I/O core never reads a clock; the
[serial driver](#a-real-converter) does it for you.

## A real converter

With the `std` feature (on by default), `serial::Bus` runs the state machines over any
`Read + Write`: the serial device of a USB level converter, or a `TcpStream` to an
Ethernet gateway. It writes each request, reads until the frame is complete or the
timeout has passed, skips line noise and a converter's echo of the request, and waits for
the whole response window during a search, so collisions are seen.

```rust
use mbus_kit::master::{Baud, Target};
use mbus_kit::serial::Bus;

// Any serial crate works; the port needs 8E1 and a short read timeout (10–100 ms).
let mut bus = Bus::new(port, Baud::B2400);
for telegram in bus.readout(Target::Primary(1))? {
    for record in telegram.application()?.records().into_iter().flatten() {
        println!("{}", record?);
    }
}
let found = bus.search()?;              // every meter by secondary address
bus.command(&master::set_primary_address(0, 5).unwrap())?;   // true on E5
```

From the command line (`brew install hupe1980/tap/mbus`); `mbus` configures the
device with `stty` (8E1, raw, 100 ms read timeout) unless you pass `--no-stty`:

```sh
mbus read /dev/ttyUSB0 1                     # primary address 1 (mbus runs stty itself)
mbus search /dev/ttyUSB0
mbus read tcp:192.168.1.50:10001 12345678    # a gateway, by secondary address
```

`Bus::slack` adds time to every timeout for network gateways (the example uses 200 ms
over TCP). `serial::Error` says whether the port failed (`Io`), the meter stayed silent
(`NoReply`), or its reply was not a telegram (`Invalid`).

## Async

The library has no async runtime dependency, and needs none: the state machines are the
async API. With tokio:

```rust
use mbus_kit::master::{Baud, Event, Readout, Target};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout_at, Duration, Instant};

async fn readout<P: AsyncReadExt + AsyncWriteExt + Unpin>(port: &mut P) -> std::io::Result<()> {
    let mut readout = Readout::new(Target::Primary(1));
    let mut buf = Vec::new();
    while let Some(request) = readout.next_request() {
        port.write_all(request.bytes()).await?;
        buf.clear();
        let deadline = Instant::now() + Duration::from_millis(request.timeout_ms(Baud::B2400).into());
        let mut chunk = [0u8; 256];
        loop {
            match timeout_at(deadline, port.read(&mut chunk)).await {
                Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                Ok(Err(e)) => return Err(e),
                _ => { readout.timeout(&buf); break; }           // timeout or closed
            }
            match readout.reply(&buf) {
                Event::NeedMore => continue,                      // frame not complete yet
                Event::Telegram(app) => println!("{:?}", app.meter()),
                _ => {}
            }
            break;
        }
    }
    Ok(())
}
```

For a long frame, extend the deadline while bytes keep arriving, as `serial::Bus` does.
