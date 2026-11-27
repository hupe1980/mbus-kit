# CI sets this for every job. A local gate that does not deny warnings cannot
# reproduce a CI failure.
export RUSTFLAGS := "-D warnings"

default: ci

# Everything CI gates on, runnable locally.
ci: test lint no-std panic-free

test:
    cargo test --all-features
    cargo run -q --example readout

lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features

# The core builds for microcontrollers with no allocator and no std.
no-std:
    cargo build --no-default-features --target thumbv7em-none-eabihf
    cargo build --no-default-features --target riscv32imac-unknown-none-elf

# A release build for a Cortex-M may contain no panic machinery at all.
panic-free:
    #!/usr/bin/env bash
    set -euo pipefail
    CARGO_PROFILE_RELEASE_LTO=false cargo build --release --target thumbv7em-none-eabihf --no-default-features
    lib=$(find target/thumbv7em-none-eabihf/release -name 'libmbus_kit*.rlib' | head -1)
    if nm "$lib" 2>/dev/null | grep -E 'core[0-9]+panicking|panic_fmt'; then
        echo "a panic path is reachable from the library"; exit 1
    fi
    echo "no panic symbols"

# Fuzz each target for a minute (nightly + cargo-fuzz).
fuzz seconds="60":
    for t in wired wireless records; do cargo +nightly fuzz run "$t" -- -max_total_time={{seconds}} -timeout=20; done

# Decode a hex capture: `just decode capture.hex`
decode file:
    cargo run -q --example decode < {{file}}

site:
    zola --root site serve
