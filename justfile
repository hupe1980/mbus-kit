# CI sets this for every job. A local gate that does not deny warnings cannot
# reproduce a CI failure.
export RUSTFLAGS := "-D warnings"

default: ci

# Everything CI gates on, runnable locally.
ci: test lint no-std panic-free

test:
    cargo test --all-features
    cargo test --no-default-features --lib
    cargo run -q --example readout
    cargo run -q --features cli --bin mbus -- decode tests/corpus/wireless/oms-n2.1-mode5.hex --key 0102030405060708090A0B0C0D0E0F11
    cargo run -q --features cli --bin mbus -- decode tests/corpus/wireless/oms-n13-mode7-profile.hex --json > /dev/null

lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# The core builds for microcontrollers with no allocator and no std.
no-std:
    cargo build --no-default-features --target thumbv7em-none-eabihf
    cargo build --no-default-features --target riscv32imac-unknown-none-elf
    cargo build --no-default-features --features oms --target thumbv7em-none-eabihf
    cargo build --no-default-features --features oms --target riscv32imac-unknown-none-elf

# A release build for a Cortex-M may contain no panic machinery at all.
panic-free:
    #!/usr/bin/env bash
    set -euo pipefail
    for features in "" "--features oms"; do
        rm -f target/thumbv7em-none-eabihf/release/libmbus_kit.rlib
        CARGO_PROFILE_RELEASE_LTO=false cargo build --release --target thumbv7em-none-eabihf --no-default-features $features
        lib=target/thumbv7em-none-eabihf/release/libmbus_kit.rlib
        if nm "$lib" 2>/dev/null | grep -iE 'panicking|panic_fmt|panic_bounds'; then
            echo "a panic path is reachable from the library ($features)"; exit 1
        fi
    done
    echo "no panic symbols (default and oms)"

# Fuzz each target for a minute (nightly + cargo-fuzz).
fuzz seconds="60":
    for t in wired wireless records oms master fragment stick serial json; do cargo +nightly fuzz run "$t" -- -max_total_time={{seconds}} -timeout=20; done

# Decode hex captures, one per line: `just decode capture.hex [--key HEX] [--json]`
decode file *args:
    cargo run -q --features cli --bin mbus -- decode {{file}} {{args}}

site:
    zola --root site serve
