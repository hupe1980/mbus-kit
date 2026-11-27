# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Wired M-Bus link layer (EN 13757-2): ACK, short, control and long frames with
  checksum, incremental parsing, encoding, `snd_nke` and `req_ud2`.
- Wireless M-Bus link layer (EN 13757-4): frame formats A and B with CRC-16/EN-13757
  per block, decoding into a caller buffer, format A encoding.
- Application layer (EN 13757-3): CI `72`, `7A`, `78`, `51`; long and short headers;
  manufacturer codes, medium, security mode from the configuration field.
- Data records: DIF/DIFE (function, storage, tariff, subunit), VIF/VIFE, plain-text
  VIF, LVAR, idle filler, manufacturer-specific data.
- Values: integers, BCD (negative and invalid), dates (types G and F), the primary VIF
  table with exact `Quantity`.
- Examples `readout` (the EN 13757-3 reference telegram) and `decode` (hex on stdin).
