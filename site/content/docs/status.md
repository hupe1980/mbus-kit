+++
title = "Status"
description = "What mbus-kit implements, what has been verified, and what comes next."
weight = 80
+++

## Implemented

| Area | Supported |
|---|---|
| Wired link layer | ACK, short, control and long frames; checksum; incremental parsing; encoding; `SND_NKE`, `REQ_UD2` |
| Wireless link layer | Frame formats A and B; CRC-16/EN-13757 per block; format A encoding |
| Application layer | CI `72`, `7A`, `78`, `51`; long and short headers; manufacturer, medium, security mode |
| Records | DIF/DIFE (function, storage, tariff, subunit); VIF/VIFE; plain-text VIF; LVAR; filler; manufacturer data |
| Values | Integers, BCD (negative, invalid), dates (types G and F), primary VIF units and exponents |

## Verified

- The **EN 13757-3 reference telegram** (the water meter example from the M-Bus
  documentation) decodes to the documented values and re-encodes byte for byte.
- CRC-16/EN-13757 matches its published check value.
- Truncated, corrupted and random input is an error, never a panic: tested in
  `cargo test` and fuzzed in CI. The `no_std` release build has no panic path.

## Not yet verified

- A corpus of real captures from several manufacturers, wired and wireless.
- **Frame format B** is implemented from the standard as understood, but no
  third-party format-B frame has confirmed it yet.

## Not yet implemented

- **OMS decryption** (security modes 5 and 7, AES-128-CBC, CMAC). Encrypted payloads
  are returned as `Body::Encrypted`, and the security mode is decoded.
- VIFE semantics: the `FD` and `FB` extension tables and combinable extensions. The
  bytes are exposed via `Vib::vifes`.
- Extended link layer (CI `8C`/`8D`), authentication and fragmentation layer (`90`).
- Secondary addressing (selection by ID) helpers and multi-telegram readout.
- An I/O shell for serial level converters and wM-Bus receiver sticks.
