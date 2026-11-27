+++
title = "Embedded builds"
description = "mbus-kit on a microcontroller: no std, no allocator, a caller-owned buffer and no panic path."
weight = 30
+++

## No std, no allocator, no dependencies

```toml
mbus-kit = { version = "0.2", default-features = false }
# with OMS decryption:
mbus-kit = { version = "0.2", default-features = false, features = ["oms"] }
```

The default build has no dependencies and never allocates, with or without `std`. The
`std` feature only adds `std::error::Error` for `Error`. `oms` adds the RustCrypto `aes`,
`cbc`, `ctr` and `cmac` crates and `zeroize`, all `no_std` and without defaults; it still
never allocates.

## Memory

Wired frames and application data are parsed in place: a `Frame` and its `Records`
borrow from the bytes you read. A wireless frame has its block CRCs removed, so
`wireless::decode` copies the data into a buffer you provide; 256 bytes hold any frame.
Decryption writes the plaintext into a second buffer of the same size;
`wireless::open` takes one buffer for both (512 bytes).

## No panic path

The library denies `panic!`, `unwrap`, `expect` and indexing by lint. CI builds it for
`thumbv7em-none-eabihf` in release mode and fails if any panic symbol is linked in. A
decoder that can abort firmware on a malformed telegram would be a denial-of-service
bug, since anyone within radio range can transmit a wireless M-Bus telegram.

The same scan runs with `--features oms`, over mbus-kit's own code. One caveat, from
upstream: the `aes` crate's software key schedule (`memshift32`, `xor_columns`) keeps
bounds checks the compiler cannot prove away, so a firmware binary built with `oms`
links a panic path from `aes`. It is unreachable for the 16-byte keys this crate passes,
but it is there; with `panic = "abort"` it costs code size, not safety.

## Targets checked in CI

- `thumbv7em-none-eabihf` (Cortex-M4/M7)
- `riscv32imac-unknown-none-elf` (e.g. ESP32-C3)
- `wasm32-unknown-unknown`

each with and without `oms`.
