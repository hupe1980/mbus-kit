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

```rust
use mbus_kit::wired::{Frame, req_ud2, snd_nke};
use mbus_kit::{Application, Body};

let mut request = [0u8; 8];
let n = snd_nke(5).encode(&mut request)?;      // reset meter 5, expect E5
let n = req_ud2(5, false).encode(&mut request)?; // ask for its data

// Feed whatever the serial port returns; `None` means "read more".
match Frame::parse(&reply)? {
    None => { /* incomplete: read more bytes */ }
    Some((Frame::Long { ci, data, .. }, _)) => {
        let app = Application::parse(ci, data)?;
        if let Body::Records(records) = app.body {
            for r in records {
                let r = r?;
                if let Some(q) = r.quantity() {
                    println!("{} = {q}", r.name());
                }
            }
        }
    }
    Some(_) => {}
}
```

Toggle the FCB (`req_ud2(addr, true)` / `false`) between successive requests to the
same meter, as EN 13757-2 requires.

## Receive a wireless meter

```rust
use mbus_kit::wireless::{Format, decode};
use mbus_kit::{Application, Body};

let mut buf = [0u8; 256];
if let Some((frame, used)) = decode(&raw, Format::A, &mut buf)? {
    let app = Application::parse(frame.ci, frame.payload)?;
    match app.body {
        Body::Records(records) => { /* as above */ }
        Body::Encrypted { security, .. } => {
            // OMS security mode 5 or 7: decryption is a later milestone.
        }
    }
}
```

`raw` starts at the L field, after the receiver has removed the radio preamble and
decoded the 3-out-of-6 or Manchester line code. Most receiver sticks do this.

## Use the value

```rust
let q = record.quantity().unwrap();   // Quantity { mantissa, exponent, unit }
q.to_string();                        // "12.565 m³"
q.to_scale(-3);                       // Some(12565): litres as an integer
q.to_scale(0);                        // None: 12.565 is not a whole number of m³
```

Each record also tells you **which** value it is: `record.dib.function`
(instantaneous, maximum, minimum, during error), `record.dib.storage` (0 = current,
higher = historic, e.g. the value at the last billing date), `record.dib.tariff` and
`record.dib.subunit`.

## Try it

```sh
cargo run --example readout                            # the EN 13757-3 reference telegram
echo "68 1F 1F 68 08 02 72 …" | cargo run --example decode
cargo run --example decode -- --wireless < frame.hex   # wM-Bus, format A
```
