# Corpus

Real and published telegrams with independently documented values. Each fixture names
its source, licence and expected values in its header; `tests/corpus.rs` explains the
format and how the expected lines were checked.

| Directory | Source | Licence | What is used |
|---|---|---|---|
| `wired/` | [libmbus](https://github.com/rscada/libmbus) `test/test-frames/` @ 9435271, expected values from the `.norm.xml` files | BSD-3-Clause, Copyright (c) 2010-2012 Raditex Control AB | frame bytes, expected values |
| `wireless/oms-*` | [OMS Specification Vol. 2 Annex N](https://oms-group.org/wp-content/uploads/2024/10/Open-Metering-System-Specification_Annex-N.pdf), Release E (2023-12) | published by the OMS Group for implementers | message examples, keys, expected values |
| `wireless/*` (others) | [wmbusmeters](https://github.com/wmbusmeters/wmbusmeters) `tests/generated_tests.xmq`, `simulations/`, `src/testinternals.cc` @ cf4a01e | GPL-3.0 | telegram bytes, keys and expected values only; no code |
| `wireless/zri-*` | [jMBus](https://github.com/qvest-digital/jmbus) `WMbusDemoMessageTest` @ 9cb761f | MPL-2.0 | telegram bytes and key only; no code |

`tests/oms.rs` also uses the jMBus `DecryptionTest` vector (MPL-2.0, bytes and keys).

Not affiliated with any of these projects. The values a peer documents are its own
decoding: where this crate deliberately differs, the fixture says why (`!` lines).

To add a capture: copy a fixture, replace the hex, document the source and the values the
meter's display or documentation shows, run `MBUS_BLESS=1 cargo test --all-features --test
corpus`, and compare every line with those values before committing.
