+++
title = "Getting started"
description = "Add mbus-kit, read a wired meter or receive a wireless one, and use exact values."
weight = 10
+++

## Where M-Bus comes from

- **Wired M-Bus** (EN 13757-2): a two-wire bus from a level converter (master) to up to
  250 meters. The master asks with `REQ_UD2`, each meter answers with `RSP_UD`. Common
  in buildings, district heating and industrial sites.
- **Wireless M-Bus** (EN 13757-4): meters send a telegram by radio every few seconds to
  minutes (mode T1 or C1 at 868 MHz, mode S1 for stationary receivers). The **OMS**
  (Open Metering System) profile defines how, including encryption.

Both carry the same application layer (EN 13757-3), which is where the readings are.

## Add the crate

```toml
[dependencies]
mbus-kit = "0.1"
# For a microcontroller:
# mbus-kit = { version = "0.1", default-features = false }
```

## Read a wired meter

Plug in a USB level converter (or reach an Ethernet gateway by TCP) and read a meter
completely, every telegram of a multi-telegram readout:

```rust
use mbus_kit::master::{Baud, Target};
use mbus_kit::serial::Bus;

// Any `Read + Write`: a serial port at 8E1 with a short read timeout, or a TcpStream.
let mut bus = Bus::new(port, Baud::B2400);
for telegram in bus.readout(Target::Primary(5))? {
    for r in telegram.application()?.records().into_iter().flatten() {
        let r = r?;
        println!("{r}");                 // "volume flow · maximum · storage 5 — 0.113 m³/h"
        if let Some(q) = r.quantity() { /* exact mantissa × 10^exponent, with its unit */ }
    }
}
```

```sh
mbus read /dev/ttyUSB0 5        # brew install hupe1980/tap/mbus
```

The driver handles timeouts, the frame count bit, retries, echoing converters and line
noise. On a microcontroller or an async runtime, drive the same state machines yourself
([Wired master](@/docs/wired-master.md)); a captured frame decodes with
`wired::Frame::parse` and `Application::parse`.

## Receive a wireless meter

Most radio meters encrypt. Enable OMS security and hand the library the meter's key:

```toml
mbus-kit = { version = "0.1", features = ["oms"] }
```

```rust
use mbus_kit::app::Authenticity;
use mbus_kit::oms::{Key, Keys};
use mbus_kit::wireless::{self, Format};

let key = Key::from_hex("0102030405060708090A0B0C0D0E0F11").unwrap(); // from the key sheet
let mut buf = [0u8; 512];
if let Some((t, _used)) = wireless::open(&raw, Format::A, &key, &mut buf)? {
    println!("{}", t.address);                       // "ELS 12345678 gas v51"
    match t.app.authenticity {
        Authenticity::Authenticated { counter } => { /* mode 7: CMAC verified; check counter > last */ }
        Authenticity::Encrypted => { /* decrypted, but anyone with the key could have sent it */ }
        Authenticity::None => { /* plaintext, or no key: anyone in range could have sent it */ }
    }
    for r in t.app.records().into_iter().flatten() {
        println!("{}", r?);                           // "volume — 28504.27 m³"
    }
}
```

- `raw` starts at the L field, after the receiver has removed the radio preamble and
  decoded the 3-out-of-6 or Manchester line code. With a USB stick, `stick::Receiver`
  gives you exactly that ([Radio receivers](@/docs/receivers.md)). Use `Format::Stripped` when the stick
  has also removed the CRCs (most do), `Format::A` or `Format::B` when it has not.
- For many meters, pass a key store: `[(Address, Key)]` works out of the box, or
  implement `Keys` for your own. A meter without a key still opens: its body stays
  `Body::Encrypted` and the address, ELL and header are there.
- Without the `oms` feature, `wireless::decode` and `frame.application()` give the same
  structure without decrypting. See [Security](@/docs/security.md) for what each level
  of authenticity means.

## Compact frames and long messages

Some meters (Kamstrup, many heat meters) send their full record layout only now and
then, and **compact frames** with the values alone in between. Keep the layouts you have
seen and expand the compact frames with them:

```rust
use mbus_kit::compact::{Format, FormatCache};
use mbus_kit::Body;

let mut formats: FormatCache<8, 64> = FormatCache::new(); // 8 layouts of ≤ 64 bytes
let mut layout = [0u8; 64];
let mut out = [0u8; 256];
match &app.body {
    Body::Records(records) => {
        if let Some(f) = Format::of(records, &mut layout) { formats.insert(&f); }
    }
    Body::Compact(c) => match c.expand(&formats, &mut out) {
        Ok(records) => { /* like any other records */ }
        Err(mbus_kit::Error::UnknownFormat(sig)) => { /* wait for a full frame */ }
        Err(e) => { /* Check::FullFrameCrc: wrong layout or corrupted */ }
    },
    _ => {}
}
```

A gateway implements `compact::Formats` over its database instead. Learn layouts only
from frames you trust as much as the compact ones (see [Security](@/docs/security.md)).

Messages longer than one frame arrive as **AFL fragments**, each with `Body::Fragment`.
Join them in a buffer you own; the last one gives the whole message, which then decrypts
and authenticates like any other:

```rust
use mbus_kit::fragment::Reassembly;

let mut buf = [0u8; 1024];
let mut message = Reassembly::new(&mut buf);       // one per meter
// for every frame of the meter:
if let Some(app) = message.push(&frame.application()?)? {
    // app.body is Body::Encrypted or Body::Records: open it as above
}
```

Fragments out of order, repeated or from another meter are reported
(`Error::UnexpectedFragment`, `FragmentFromOtherMeter`) and the message in progress is
kept, so the master can request the missing one again.

## Use the value

Every record prints as one line, with defaults (instantaneous, storage 0, tariff 0)
left out and meter-supplied text escaped:

```rust
println!("{record}");   // "volume flow · maximum · storage 5 — 0.113 m³/h"
```

For code, ask what the value **is**, then for the number:

```rust
use mbus_kit::{Kind, Value};

let d = record.description();        // kind, unit, exponent, table
match d.kind {
    Kind::Volume => { /* … */ }
    Kind::ErrorFlags => { /* bits in record.value */ }
    Kind::FirmwareVersion => { /* … */ }
    _ => {}
}
let q = record.quantity().unwrap();   // Quantity { mantissa, exponent, unit }
q.to_string();                        // "12.565 m³"
q.to_scale(-3);                       // Some(12565): litres as an integer
q.to_scale(0);                        // None: 12.565 is not a whole number of m³
```

`quantity()` applies correction factors (VIFE `70`–`77`, `7D`) exactly and returns
`None` when a VIFE changes the meaning in a way the crate does not model ("per hour",
"date of", a record error, manufacturer-specific). The modifiers themselves are typed:

```rust
for m in record.vib.modifiers() {
    // Modifier::Per(Per::Hour), Modifier::Limit { upper: true }, Modifier::Scale(-1), …
    println!("{m}");
}
```

Each record also tells you **which** value it is: `record.dib.function`
(instantaneous, maximum, minimum, during error), `record.dib.storage` (0 = current,
higher = historic, e.g. the value at the last billing date), `record.dib.tariff` and
`record.dib.subunit`. Dates and times arrive as `Value::Date`, `Value::Time` and
`Value::DateTime` (types G, J, F and I); a date the meter flags invalid is still a
value, with `is_valid()` false.

## Try it

For programs, every `mbus` command speaks JSON Lines ([JSON and the mbus tool](@/docs/json.md)).

```sh
cargo run --example readout                            # the EN 13757-3 reference telegram
brew install hupe1980/tap/mbus   # or: cargo install mbus-kit --features cli
echo "68 1F 1F 68 08 02 72 …" | mbus decode         # wired or wireless, format detected
mbus decode frames.hex --key <32 hex> --json        # JSON lines
mbus search /dev/ttyUSB0                           # every meter on a real bus
mbus listen /dev/ttyUSB0 --stick im871a --mode t1
```
