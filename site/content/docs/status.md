+++
title = "Status"
description = "What mbus-kit implements, what has been verified against real meters, and what comes next."
weight = 80
+++

## Implemented

| Area | Supported |
|---|---|
| Wired link layer | ACK, short, control and long frames; checksum; incremental parsing; resynchronising scan; encoding |
| Wired master | Readout with FCB and retries, multi-telegram; secondary addressing with wildcards and fabrication number; bus search; set address, baud rate, clock, application reset; a blocking driver over any port or TCP gateway |
| JSON and the `mbus` tool | One JSON object per telegram (no_std); `mbus decode/read/search/listen` |
| Receiver sticks | IMST iM871A and iU891A, Amber AMB8465: telegrams with RSSI, receive-mode commands |
| Wireless link layer | Frame formats A and B with CRC-16/EN-13757 per block; frames with CRCs already removed; format A encoding |
| Layers | ELL (CI `8C`–`8F`), AFL (CI `90`) with fragment reassembly, transport CIs `72`, `7A`, `78`, `51`, `5A`, `5B`, `6E`–`70`, `71`, `74`, `75`, `80`, `8A`, `8B` |
| Compact frames | CI `79`, `7B`, `73` expanded with a caller-owned layout cache and checked by the full-frame CRC; format frames CI `69`, `6A`, `6B` |
| OMS security (`oms`) | Mode 5, mode 7 with CMAC, mode 10 (AES-CCM), ELL AES-CTR; authenticity levels; zeroized keys |
| Records | DIF/DIFE; VIF with the primary, `FB`, `FD`, `FD FD` and combinable tables; plain-text VIF; LVAR; filler; manufacturer data |
| Values | Signed and unsigned integers, BCD (negative, invalid), IEEE reals as exact decimals, dates and times (types G, J, F, I), time offsets (type M), compact profiles (VIFE `13`, `1E`, `1F`) with dates, exact corrections |

## Verified

- **41 real and published telegrams** in `tests/corpus/`: 15 wired frames from
  libmbus (Kamstrup, Elster, Sensus, Siemens, Itron, Elvaco, Relay, EDC, EMU, ABB,
  Engelmann) and 26 wireless telegrams from the OMS Annex N, wmbusmeters and jMBus
  (Sensus, Techem, Qundis, Engelmann, Kamstrup, Zenner, Bmeters, Baylan, Weptech,
  Gran-System, Aventies, ebeco, Siemens). Every record matches the documented value;
  where the crate deliberately differs (manufacturer VIFEs, invalid BCD, reals), the
  fixture names the deviation.
- **OMS security**: OMS Vol. 2 Annex N examples for mode 5 (short and long header,
  partial encryption), mode 7 with AFL (radio, radio adapter, wired), and real Kamstrup
  ELL, Techem, Qundis, Sensus and ebeco telegrams with their keys. Every single-bit
  change to an authenticated mode-7 telegram or an ELL payload is rejected.
- **Mode 10** (AES-CCM): OMS Annex N N.2.6 and a real Kamstrup water meter with its
  corrupted twin; every single-bit change is rejected.
- **Fragments**: OMS Annex N N.3 (radio, mode 7 with CMAC), N.4 (wired, mode 5) and N.5
  (radio, mode 10), three fragments each. Every single-bit change to the N.3 pieces or
  its MAC is rejected. No other open implementation reassembles AFL fragments.
- **Compact frames**: the Kamstrup format-B compact frame and its corrupted twin (only
  the full-frame CRC catches it), kamwater full/compact pairs, the EN 13757-3 G.5
  example. **Compact profiles and type M**: OMS Annex G G.6/G.7, a Techem heat cost
  allocator, and N.13, whose 2 × 46 readings match the Annex table.
- The **wired master** against a simulated bus with realistic collisions (overlapping
  `E5`s, garbled replies): every meter of buses of up to 250 meters with clustered IDs is
  found; command frames match the m-bus.com, libmbus and pyMeterBus reference frames
  byte for byte.
- **Receiver sticks**: real captures from an iM871A (wmbusmeters issues #750, #1055,
  #1517), an AMB8465 (#1049) and an iU891A, and the vendors' command examples; corrupted
  and truncated stick frames cost only themselves. The **serial driver** against a
  simulated converter (echo, noise, 1-byte chunks, silence, random and endless replies)
  and a real TCP socket.
- An **independent review** of every module against OMS Vol. 2 5.0.1 and Annex N found
  six defects (mode-10 tail, stripped MAC, mode 13 as plaintext, a false ACK in chunked
  wired replies, type-F years, format-B lengths); each is fixed with a regression test.
- **JSON**: every corpus telegram parses with a strict RFC 8259 parser, its numbers equal
  the exact quantities; fuzzed.
- The **EN 13757-3 reference telegram** decodes to the documented values and re-encodes
  byte for byte. CRC-16/EN-13757 and AES-CMAC match their published check values.
- Truncated, corrupted and random input is an error, never a panic: tested and fuzzed
  (wired, wireless, records, oms, master, fragment, stick, serial, json). The `no_std` release build has no panic path in the
  library's code.

## Not yet verified

- **Frame format B with two CRCs** (frames over 128 bytes): only the single-CRC case is
  confirmed by a real frame.
- **Mode 7 from a real meter**: only the OMS Annex N examples are public.
- **Real hardware on this project's bench**: the serial driver and the stick decoders
  are tested against simulations and recorded captures from other people's sticks.
- **An ELL-encrypted compact frame**: every public Kamstrup compact frame is published
  already decrypted; the path is the tested ELL decryption followed by the tested
  compact parser.
- EN 13757-3:2018 itself is not public; its tables were taken from the OMS data point
  list (Annex B) and cross-checked against four implementations.

## Not yet implemented

- Mode 13 (TLS) and the bidirectional key exchange.
- Compact frames inside TPL mode 5 or 7 (no public vector settles their CRC).
- Data types K and L (daylight saving, listening window): no telegram in the corpus.
