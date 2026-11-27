# mbus-kit

**M-Bus and wireless M-Bus (EN 13757) for Rust: read heat, water and gas meters with a sans-I/O, `no_std`, allocation-free core.**

[![CI](https://github.com/hupe1980/mbus-kit/actions/workflows/ci.yml/badge.svg)](https://github.com/hupe1980/mbus-kit/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)
[![unsafe](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/hupe1980/mbus-kit/blob/main/src/lib.rs)

**[Documentation](https://hupe1980.github.io/mbus-kit/) · [API reference](https://docs.rs/mbus-kit)**

> 🚧 **Pre-release.** Checked against a corpus of 41 real and published telegrams from
> 20 manufacturers (libmbus, wmbusmeters, jMBus, OMS Annex N), wired and wireless,
> plain and encrypted. The API may still change before 1.0.

Heat, water, gas and heat-cost-allocator meters across Europe speak M-Bus: wired
(EN 13757-2, two-wire bus) or by radio (wireless M-Bus, EN 13757-4, and its OMS
profile). Both carry the same application layer (EN 13757-3). mbus-kit turns those
bytes into exact, fully described readings:

```rust
use mbus_kit::wired::Frame;
use mbus_kit::Application;

fn on_response(bytes: &[u8]) -> Result<(), mbus_kit::Error> {
    let Some((Frame::Long { ci, data, .. }, _)) = Frame::parse(bytes)? else { return Ok(()) };
    for record in Application::parse(ci, data)?.records().into_iter().flatten() {
        println!("{}", record?); // "volume flow · maximum · storage 5 — 0.113 m³/h"
    }
    Ok(())
}
```

And by radio, encrypted (feature `oms`):

```rust,ignore
use mbus_kit::oms::Key;
use mbus_kit::wireless::{self, Format};

let key = Key::from_hex("0102030405060708090A0B0C0D0E0F11").unwrap();
let mut buf = [0u8; 512];
if let Some((telegram, _)) = wireless::open(&raw, Format::A, &key, &mut buf)? {
    println!("{} · {:?}", telegram.address, telegram.app.authenticity); // Encrypted, or Authenticated { counter }
    for record in telegram.app.records().into_iter().flatten() {
        println!("{}", record?); // "volume — 28504.27 m³"
    }
}
```

## What it does

| Layer | Status |
|---|---|
| **Wired link layer** (EN 13757-2): ACK, short, control and long frames, checksum; resynchronising scan for noisy lines | ✅ parse and encode, incremental (asks for more bytes) |
| **Wired master** (sans-I/O): complete readout with FCB and retries, secondary addressing with wildcards, bus search, set address, baud rate, clock, application reset | ✅ tested against a simulated bus of 250 meters |
| **Real converters** (`serial::Bus`, std, no dependency): the master over any `Read + Write` — a USB level converter or a TCP gateway; echo, noise and collisions handled, every wait bounded | ✅ simulated converter and a real socket |
| **JSON** (`json`, `no_std`, no allocation): one object per telegram — meter, authenticity, every record with exact value, unit, storage, tariff | ✅ every corpus telegram checked by a strict parser; fuzzed |
| **`mbus` tool** (feature `cli`, no dependency): `decode`, `read`, `search`, `listen`, text or `--json`, sets up serial ports itself | ✅ |
| **Receiver sticks** (`stick`, `no_std`): IMST iM871A, IMST iU891A, Amber AMB8465 — telegrams with RSSI, receive-mode commands | ✅ checked against real captures |
| **Wireless link layer** (EN 13757-4): frame formats A and B with CRC-16/EN-13757 per block, or CRCs already removed by the receiver | ✅ decode; encode format A |
| **Extended link layer** (CI `8C`–`8F`) and **authentication and fragmentation layer** (CI `90`), with **fragment reassembly** into your buffer | ✅ checked against OMS Annex N N.3–N.5 |
| **Compact frames** (CI `79`/`7B`/`73`) and format frames: values without their layout, expanded with a layout cache you own, checked by the full-frame CRC | ✅ |
| **Transport layer**: CI `72`/`7A`/`78`/`51`, commands `5A`/`5B`, errors `6E`–`70`, alarms `71`/`74`/`75`, `80`/`8A`/`8B` | ✅ |
| **OMS security** (feature `oms`): mode 5 (AES-CBC), mode 7 (KDF, AES-CBC, **CMAC verified**), mode 10 (AES-CCM, **tag verified**), ELL AES-CTR; keys zeroized, never printed | ✅ checked against OMS Annex N and real meters |
| **Data records**: DIF/DIFE, VIF/VIFE, plain-text VIF, LVAR, filler, manufacturer data | ✅ lazily, without allocating |
| **Value information**: primary, `FB`, `FD`, `FD FD` tables and every combinable VIFE (EN 13757-3:2018), typed | ✅ `Kind`, `Modifier`; correction factors applied exactly |
| **Values**: integers (signed and unsigned), BCD (incl. negative and invalid), IEEE reals (exactly, as decimals), dates and times (types G, J, F, I), time offsets (type M) | ✅ exact `Quantity`, never floats |
| **Compact profiles** (VIFE `13`/`1E`/`1F`): monthly or interval history as readings with their dates or offsets | ✅ checked against OMS Annex G and N.13 |
| Mode 13 (TLS), data types K/L | Not yet |

## Design

- **Sans-I/O.** No serial port, no radio driver, no clock. Feed bytes from an M-Bus level
  converter, a wM-Bus receiver stick or a capture file.
- **`no_std`, no allocator.** The default build has **no dependencies**; `oms` adds only
  the [RustCrypto](https://github.com/RustCrypto) AES modes and `zeroize`. Records borrow from the frame. The library's
  release build for a Cortex-M contains no panic path, checked in CI.
- **Exact.** Readings are `mantissa × 10^exponent` with their unit, never floats.
  Correction factors are applied exactly; a VIFE the crate cannot model yields no
  quantity rather than a wrong one.
- **Honest about radio.** Every telegram says what was verified: nothing, decrypted, or
  authenticated by CMAC or CCM tag. A CRC or a mode-5 decryption is not authentication: both can
  be forged or modified, and the tests show how.
- **Never panics on input.** Malformed, truncated and random bytes are errors. Fuzzed.

## Try it

```sh
cargo run --example readout                                   # the reference telegram
cargo run --example master                                    # read a meter, search a bus (simulated)
brew install hupe1980/tap/mbus                                 # the `mbus` tool (macOS, Linux)
cargo install mbus-kit --features cli                         # …or from source; binaries on the releases page
mbus read /dev/ttyUSB0 5                                       # a wired meter (port set up for you)
mbus listen /dev/ttyUSB0 --stick im871a --mode t1 --key <32 hex>   # radio meters through a stick
mbus decode captures.hex --json                                # captures as JSON lines
just decode tests/corpus/wireless/oms-n13-mode7-profile.hex    # 46-step load profile, type M
```

`mbus decode` takes one telegram per line, recognises wired and wireless frames and the
radio format (A, B, CRCs removed) by itself, learns record layouts from full frames to
expand later compact frames, and joins AFL fragments on consecutive lines. With `--json`,
every command writes one JSON object per telegram:

```json
{"meter":{"manufacturer":"ELS","id":"12345678","version":51,"medium":"gas","medium_code":3},"ci":"7A","access":117,"status":0,"authenticity":"authenticated","counter":2739,"body":"records","records":[{"name":"volume","function":"instantaneous","storage":0,"tariff":0,"subunit":0,"modifiers":[],"value":28504.27,"unit":"m3","encrypted":true,"text":"volume — 28504.27 m³"}, …]}
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
