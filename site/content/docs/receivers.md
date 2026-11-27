+++
title = "Radio receivers"
description = "Listen to wireless M-Bus meters with an IMST iM871A or iU891A or an Amber AMB8465 stick: one decoder for all of them, telegrams with RSSI, ready to decrypt."
weight = 17
+++

## Sticks

A wM-Bus USB stick receives the radio frames, checks their CRCs and passes them on over a
serial line in its own protocol. `stick::Receiver` speaks three of those protocols. It
is sans-I/O, `no_std` and allocation-free, so it also runs on a microcontroller wired to
a radio module.

| Stick | `Stick::` | Serial | Modes | RSSI |
|---|---|---|---|---|
| IMST iM871A, iM170A | `Im871a` | 57600 8N1 | S1, T1, C1, C1+T1 (firmware ≥ 1.4) | approximate (IMST gives no formula) |
| IMST iU891A, iM891A, iU893A | `Iu891a` | 115200 8N1 | S1, T1, C1, C1+T1 | dBm, reported by the stick |
| Amber/Würth AMB8465, AMB8665 | `Amb8465` | 9600 8N1 | T1, C1, C1+T1 | dBm, vendor formula |

## Listen

```rust
use mbus_kit::stick::{LinkMode, Message, Receiver, Stick};
use mbus_kit::wireless::{self, Format};

let stick = Stick::Im871a;
port.write_all(stick.listen(LinkMode::T1).unwrap().bytes())?;   // RAM only, once per power-up
let mut rx = Receiver::new(stick);
let mut chunk = [0u8; 256];
loop {
    let n = port.read(&mut chunk)?;
    rx.extend(&chunk[..n]);
    while let Some(message) = rx.next_message() {
        match message {
            Ok(Message::Telegram(t)) => {
                let mut buf = [0u8; 512];
                if let Some((telegram, _)) = wireless::open(t.frame, Format::Stripped, &key, &mut buf)? {
                    println!("{} · {:?} dBm · {:?}", telegram.address, t.rssi_dbm, telegram.app.authenticity);
                }
            }
            Ok(Message::Response { .. }) => {}          // the stick confirming a command
            Err(e) => eprintln!("skipped: {e}"),        // noise or a corrupted stick frame
        }
    }
}
```

`t.frame` always starts at the L field with the link-layer CRCs removed, whichever stick
it came from: hand it to `wireless::open` or `wireless::decode` with
`Format::Stripped`. A corrupted stick frame comes out as an error and costs only itself:
the next frame decodes. `extend` returns how much it took; it takes everything unless its
640-byte buffer is full of an unfinished frame.

## Try it

```sh
mbus listen /dev/ttyUSB0 --stick im871a --mode t1 --key <32 hex>
mbus listen /dev/ttyACM0 --stick iu891a --mode c1t1 --json      # JSON lines with rssi_dbm
mbus listen - --stick im871a < capture.bin                      # a recorded stream
```

`mbus` configures the serial device with `stty` (8N1 at the stick's rate); pass
`--no-stty` if you set it up yourself. `mbus decode` also takes the text lines of
[rtl-wmbus](https://github.com/xaelsouth/rtl-wmbus) (`T1;1;1;…;0x2e44…`), for an
RTL-SDR dongle instead of a stick.

## Amber sticks: once, then listen

The Amber sticks deliver telegrams in a framed format only in *command mode with RSSI*;
out of the box they send bare bytes that cannot be resynchronised after a lost byte. Run
the setup **once per stick**. It writes the stick's flash, which wears:

```sh
mbus listen /dev/ttyUSB0 --stick amb8465 --mode c1t1 --setup
```

In code: `for c in Stick::Amb8465.setup() { port.write_all(c.bytes())?; }`. After that,
`Stick::listen` on every start, which only changes RAM.

## What a stick proves

Nothing about the meter. The stick checks the radio CRCs, which detect noise, not
forgery. Its RSSI says how loud a transmitter was, not who it was. The iU891A can
decrypt with keys stored in it and then sets `decrypted_by_stick`; the crate reports
that, but authenticity still comes only from its own checks
([Security](@/docs/security.md)). A stick telegram is a radio telegram.
