# mbus-kit

**M-Bus and wireless M-Bus (EN 13757) for Rust: read heat, water and gas meters with a sans-I/O, `no_std`, allocation-free core.**

[![CI](https://github.com/hupe1980/mbus-kit/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/mbus-kit/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/hupe1980/mbus-kit/blob/main/src/lib.rs)

**[Documentation](https://hupe1980.github.io/mbus-kit/) · [API reference](https://docs.rs/mbus-kit)**

> 🚧 **Pre-release.** Decodes the EN 13757-3 reference telegram exactly and survives
> malformed input. Not yet checked against a corpus of real meters, and OMS
> encryption (security modes 5 and 7) is detected but not decrypted yet.

Heat, water, gas and heat-cost-allocator meters across Europe speak M-Bus: wired
(EN 13757-2, two-wire bus) or by radio (wireless M-Bus, EN 13757-4, and its OMS
profile). Both carry the same application layer (EN 13757-3). mbus-kit turns those
bytes into exact readings:

```rust
use mbus_kit::wired::Frame;
use mbus_kit::{Application, Body};

fn on_response(bytes: &[u8]) -> Result<(), mbus_kit::Error> {
    let Some((Frame::Long { ci, data, .. }, _)) = Frame::parse(bytes)? else { return Ok(()) };
    if let Body::Records(records) = Application::parse(ci, data)?.body {
        for record in records {
            let r = record?;
            if let Some(q) = r.quantity() {
                println!("{} = {q}", r.name()); // "volume = 12.565 m³", exact, never a float
            }
        }
    }
    Ok(())
}
```

## What it does

| Layer | Status |
|---|---|
| **Wired link layer** (EN 13757-2): ACK, short, control and long frames, checksum; `SND_NKE`, `REQ_UD2` | ✅ parse and encode, incremental (asks for more bytes) |
| **Wireless link layer** (EN 13757-4): frame formats A and B, CRC-16/EN-13757 per block | ✅ decode; encode format A |
| **Application layer** (EN 13757-3): CI `72`/`7A`/`78`/`51`, long and short header, manufacturer, medium, security mode | ✅ |
| **Data records**: DIF/DIFE (function, storage, tariff, subunit), VIF/VIFE, plain-text VIF, LVAR, filler, manufacturer data | ✅ lazily, without allocating |
| **Values**: integers, BCD (incl. negative and invalid), dates (types G and F), primary VIF units and exponents | ✅ exact `Quantity` |
| OMS decryption (security modes 5 and 7, AES-128) | Planned, behind an opt-in feature |
| VIFE semantics (`FD`/`FB` tables, combinable extensions) | Planned (extensions are exposed raw) |
| Extended link layer (CI `8C`/`8D`), authentication and fragmentation layer (CI `90`) | Planned |
| Real-meter capture corpus and differential tests | Next |

## Design

- **Sans-I/O.** No serial port, no radio driver, no clock. Feed bytes from an M-Bus level
  converter, a wM-Bus receiver stick or a capture file.
- **`no_std`, no allocator, no dependencies.** Records borrow from the frame. The release
  build for a Cortex-M contains no panic path, checked in CI.
- **Exact.** Readings are `mantissa × 10^exponent` with their unit, never floats.
  32-bit reals are passed through as raw bits for the caller to decide.
- **Never panics on input.** Malformed, truncated and random bytes are errors.

## Try it

```sh
cargo run --example readout                               # the reference telegram
echo "68 1F 1F 68 08 02 72 …" | cargo run --example decode  # your own capture
cargo run --example decode -- --wireless < frame.hex        # a wM-Bus frame (format A)
```

## Part of Open Utility Stack

mbus-kit is a protocol crate for the
[Open Utility Stack](https://hupe1980.github.io/open-utility-stack/): water, gas and heat
meters for [imsys](https://github.com/hupe1980/imsys) (the gateway's meter side) and
[ems](https://github.com/hupe1980/ems), and the basis for water and heat billing. Its
siblings for meters are [sml-kit](https://github.com/hupe1980/sml-kit) and
[dlms-cosem-kit](https://github.com/hupe1980/dlms-cosem-kit).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Not affiliated with CEN, the OMS Group or any meter manufacturer.
