+++
title = "Embedded builds"
description = "mbus-kit on a microcontroller: no std, no allocator, a caller-owned buffer and no panic path."
weight = 30
+++

## No std, no allocator, no dependencies

```toml
mbus-kit = { version = "0.1", default-features = false }
```

The crate has no dependencies and never allocates, with or without `std`. The `std`
feature only adds `std::error::Error` for `Error`.

## Memory

Wired frames and application data are parsed in place: a `Frame` and its `Records`
borrow from the bytes you read. A wireless frame has its block CRCs removed, so
`wireless::decode` copies the data into a buffer you provide; 256 bytes hold any frame.

## No panic path

The library denies `panic!`, `unwrap`, `expect` and indexing by lint. CI builds it for
`thumbv7em-none-eabihf` in release mode and fails if any panic symbol is linked in. A
decoder that can abort firmware on a malformed telegram would be a denial-of-service
bug, since anyone within radio range can transmit a wireless M-Bus telegram.

## Targets checked in CI

- `thumbv7em-none-eabihf` (Cortex-M4/M7)
- `riscv32imac-unknown-none-elf` (e.g. ESP32-C3)
- `wasm32-unknown-unknown`
