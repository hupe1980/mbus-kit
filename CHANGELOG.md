# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-11-27

### Added

- **JSON** (spec 006, `mbus_kit::json`, `no_std`, no allocation): `Json::new(&app)`
  (`.rssi(dbm)`) writes one object per telegram: meter, CI, access, status, RSSI,
  authenticity and counter, body kind, and every record with name, function, storage,
  tariff, subunit, modifiers, an exact decimal value with an ASCII unit code, `encrypted`
  and its display text; compact profiles as structured `profile` objects. `JsonRecord`,
  `JsonAddress`, `JsonString`. Fuzz target `json`.
- **The `mbus` tool** (feature `cli`): `decode`, `read`, `search`, `listen`, `--json`;
  configures serial devices with `stty`. Replaces the `decode`, `converter` and `listen`
  examples.
- `Application::expand`: a compact frame as a full application.
- Releases: a version tag builds `mbus` for Linux (x86_64, aarch64; static musl), macOS
  (Apple silicon) and Windows with SHA256 checksums, a CycloneDX SBOM and build
  provenance attestations, updates the Homebrew formula (`brew install
  hupe1980/tap/mbus`, tested on macOS and Linux after publishing) and publishes the crate
  through crates.io trusted publishing.

- **Real converters** (spec 005, `mbus_kit::serial`, feature `std`, no dependency):
  `Bus` drives `Readout`, `Search` and commands over any `Read + Write` (a level
  converter's serial device, a `TcpStream` to a gateway): first-byte deadline with
  `slack`, end on a complete frame or a quiet line, a hard end and a 512-byte cap per
  reply, converter echo removed, line noise skipped; `Telegram`, `Found { meters,
  ambiguous, complete }`, `serial::Error`. Example `converter`.
- **Receiver sticks** (`mbus_kit::stick`, `no_std`): `Receiver` decodes the IMST iM871A
  (HCI 1.6), IMST iU891A (gateway HCI 2.4, SLIP) and Amber AMB8465 (command mode) serial
  protocols into stripped telegrams with RSSI, timestamp and link mode; `Stick::listen`,
  `ping`, `setup`, `baud`. Checked against real captures. Example `listen`; fuzz targets
  `stick` and `serial`. `crc::crc16_x25`.
- `Search::max_selections` (default 20 000) and `Search::is_complete`: a stuck or
  babbling line no longer makes a search run for 10⁸ selections.
- The `decode` example reads rtl-wmbus lines.

- **Compact frames** (spec 004, `mbus_kit::compact`): CI `79`/`7B`/`73` as
  `Body::Compact`, format frames CI `69`/`6A`/`6B` as `Body::Format`; `Format::of`
  computes a full frame's layout and signature, `Format::received` checks a received one;
  the `Formats` lookup trait for your own cache and the fixed-size `FormatCache<N, B>`;
  `Compact::expand` rebuilds the records into your buffer and verifies the full-frame
  CRC. Errors `UnknownFormat(signature)`, `InvalidFormat`, `Check::FullFrameCrc`.
- **AFL fragment reassembly** (`mbus_kit::fragment::Reassembly`): fragments are
  `Body::Fragment`; `push` checks order, sender, announced length and MAC length and
  returns the whole message after the last fragment, which then decrypts and authenticates
  like an unfragmented one (the mode-7 CMAC covers the whole message). Errors
  `UnexpectedFragment { expected, found }`, `FragmentFromOtherMeter`. `Afl::fragment_id`.
  Checked against OMS Annex N N.3, N.4 and N.5.
- **Mode 10** (AES-128-CCM, feature `oms`, adds `ccm`): `Security::Mode10`, CFE, key
  version, TPL counter (`ShortHeader::counter`) and tag; `Authenticated { counter }` only
  after the tag verifies (`Check::Tag`), plaintext erased on failure. OMS Annex N N.2.6
  and a real Kamstrup meter.
- **Compact profiles** (`mbus_kit::profile`): `Record::profile()`, `Profile::values(base)`,
  `Profile::offset(k)`, `Profile::date(k, base)`, `Increment`, `Spacing`, `Offset`;
  OMS Annex G G.6/G.7, N.13 and a Techem heat cost allocator.
- **Data type M** (relative time offset) as `Value::Duration`, shown as
  `3 s before reception`. `Date::add_months` (month ends kept; `None` for an invalid date or a year out of range).
- The `decode` example expands compact frames with layouts learnt from earlier full
  frames and joins fragments on consecutive lines. Fuzz target `fragment`; corpus fixture
  OMS Annex N N.13.

- **Wired master** (spec 003, `mbus_kit::master`), sans-I/O: `Readout` (SND_NKE or
  deselect-select, `REQ_UD2` until no more records follow, FCB per EN 13757-2, retries,
  telegram cap), `Search` (wildcard scan with collision detection; meters sharing an ID
  reported as `Event::Ambiguous`), `SecondaryAddress` (wildcard patterns), command frames
  (`select`, `select_enhanced`, `deselect`, `set_primary_address`, `application_reset`,
  `set_baud`, `set_time`, `request_all_data`, `link_reset`), `Request` with timeouts in bit
  times; `wired::scan` for noisy lines; `DateTime::new`, `DateTime::to_type_f`; example
  `master`; fuzz target `master`.
- Energy and power display with SI prefixes, still exact: `218.37 kWh`; `{:#}` keeps the
  base unit.

- **OMS security** behind the opt-in feature `oms` (spec 002): security mode 5
  (AES-128-CBC), mode 7 (KDF, AES-128-CBC, AFL CMAC verified before decryption, constant
  time) and the ELL's AES-128-CTR with payload CRC. `oms::Key` (zeroized on drop, never
  printed, `from_hex`) and the `oms::Keys` lookup trait. Checked against OMS Vol. 2
  Annex N, Kamstrup, Techem, Qundis, Sensus and jMBus vectors; every single-bit flip of
  the mode-7 and ELL vectors is rejected.
- `Authenticity` on every `Application`: `None`, `Encrypted` (confidential, not
  integrity-protected: mode 5 and the ELL are malleable), or `Authenticated { counter }`
  (only after a verified CMAC). `Record::from_ciphertext` separates decrypted records
  from the unprotected plain tail of a partially encrypted payload.
- `wireless::open(raw, format, keys, buf)`: one call from radio bytes to address,
  authenticity and records; `wireless::Frame::application()`.
- The full layer chain: ELL (CI `8C`–`8F`), AFL (CI `90`), and the transport CIs `5A`,
  `5B`, `6E`–`70`, `71`, `74`, `75`, `80`, `8A`, `8B` with `Body::ApplicationError`,
  `Body::Alarm`, `Body::Empty`.
- `wireless::Format::Stripped` for receivers that already removed the CRCs.
- **Every record described** (spec 001): the `FD`, `FD FD`, `FB` and combinable VIFE
  tables of EN 13757-3:2018 as typed `Kind`, `Description` and `Modifier`;
  multiplicative corrections applied exactly; `quantity()` returns `None` for any VIFE
  that changes unit or meaning in a way the crate does not model.
- Data types J (time) and I (date-time with seconds, weekday, week, leap year);
  `Date::is_valid`, `DateTime::is_valid`; the EN 13757-3 century rule.
- Unsigned integers (data type C, `FC 11`) as `Value::UInt`; LVAR `F0`–`F6`.
- IEEE 754 reals as exact decimals (`Value::as_decimal`, `Record::quantity`), and
  `Quantity::round` (half to even) and `Quantity::significant` for explicit rounding.
- `Display` for `Record` (one line: "volume flow · maximum · storage 5 — 0.113 m³/h"),
  `Value`, `Kind`, `Modifier`, `Address`, `Date`, `Time`, `DateTime`. Meter-supplied
  text is escaped (`\xNN`), so a crafted telegram cannot put control sequences into a
  terminal or log.
- Plain-text VIF `FC` with VIFEs in the standard order, with a fallback for meters that
  put the text first (Elvaco CMa10).
- A corpus of 40 real and published telegrams (libmbus, wmbusmeters, jMBus, OMS Annex N),
  wired and wireless, with every deviation from the standard named and tested.
- The `decode` example: one telegram per line, wired/wireless and format detected, keys
  with `--key` or in the file; fuzz target `oms`; a no-floats test.
- Media names for repeaters, radio converters, gateways, valves, breakers.

### Changed (breaking)

- **Fixes from an independent review**: a mode-10 telegram with bytes between the
  encrypted part and the tag is refused unless the tag covers them (it was reported
  `Authenticated` with appended records); an AFL announcing a MAC that is absent fails
  with `Check::Mac` (it opened as `Encrypted`); security modes other than 5 and 7 stay
  `Encrypted` (mode 13 with a short payload was read as records); `wired::scan` waits for
  an incomplete frame instead of taking an `E5` in its data for an acknowledgement (a
  chunked reply ended a readout with no telegram); `DateTime::new` stops at 2299 (type
  F); format B with L + 1 of 129 or 130 is `InvalidLength`.
- The `decode`, `converter` and `listen` examples are gone: use `mbus`.

- `master::response_timeout_bits` is `response_timeout_ms(baud)` and `Request` has no
  `timeout_bits`: the timeout was counted in bit times at 2400 Bd, so its 50 ms shrank at
  higher rates (47 ms instead of 85 ms at 9600 Bd). `Request::timeout_ms(baud)` is now
  330 bit times at `baud` plus 50 ms.

- `Body` has the variants `Compact`, `Format` and `Fragment`; an AFL with a fragment ID
  other than 0 ends parsing with `Body::Fragment` instead of misreading the next piece
  as a transport layer. An AFL MAC without MCL (a last fragment) takes its length from
  the AFL length.

- `wireless::Address` is now `mbus_kit::Address` and is shared with `LongHeader`, which
  is `{ address, short }`.
- `Body::Encrypted` is a tuple variant holding `Encrypted`; `Application` has `ell`,
  `afl`, `authenticity`, `meter()`, `records()`; `ShortHeader` has `config_ext`.
- `Vib` moved to `mbus_kit::vif` with accessors (`vif()`, `plain_text()`);
  `Vib::describe` and `record::Description` are replaced by `Vib::description` /
  `Record::description` and `vif::Description`.
- `Record::quantity` applies corrections and refuses unmodelled VIFEs; it also converts
  reals exactly (D-012, superseding D-003).
- `DateTime` has `time: Time`, `has_seconds`, `leap_year`, `weekday`, `week`.
- A mode-5 configuration with zero encrypted blocks is plaintext (OMS Vol. 2 p. 62).

### Initial

- Wired M-Bus link layer (EN 13757-2): ACK, short, control and long frames with
  checksum, incremental parsing, encoding, `snd_nke` and `req_ud2`.
- Wireless M-Bus link layer (EN 13757-4): frame formats A and B with CRC-16/EN-13757
  per block, decoding into a caller buffer, format A encoding.
- Application layer (EN 13757-3), data records and values; the EN 13757-3 reference
  telegram decodes and re-encodes byte for byte.

[Unreleased]: https://github.com/hupe1980/mbus-kit/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hupe1980/mbus-kit/releases/tag/v0.1.0
