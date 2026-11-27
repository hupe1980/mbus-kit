# Threat model

## What mbus-kit trusts

Nothing it decodes. Bytes come from a wired bus, a radio receiver anyone in range can
transmit to, or a capture file.

## What it defends against

| Threat | Defence |
|---|---|
| Malformed or crafted input crashes the process | No `unsafe`; panics, `unwrap` and indexing denied by lint; panic-symbol scan of the no_std release build; fuzz targets on wired, wireless, records, oms, master and fragment |
| Input exhausts memory | No allocation; wireless frames, expanded compact frames and reassembled fragments are copied into caller buffers, and what does not fit is an error |
| Input exhausts CPU | DIFE and VIFE chains are capped at 10; every loop consumes input; each layer (ELL, AFL, TPL) appears at most once |
| A forged or modified mode-7 telegram is accepted | The AFL CMAC is verified with the per-message key over the transport layer before decryption, compared in constant time; any failure is an error, never records |
| A forged or modified mode-10 telegram is accepted | The AES-CCM tag is verified before any record exists; the plaintext is erased on failure |
| A forged or replaced fragment is accepted as part of an authenticated message | The last fragment's CMAC covers the whole reassembled message, its counter and length; fragment order, sender and the announced length are checked |
| A compact frame decodes with the wrong layout | The full-frame CRC is checked after expansion; unknown layouts are an error, never guessed |
| A babbling, stuck or colliding device holds the wired master forever | `serial::Bus` bounds every reply in time and size; `Search::max_selections` ends a search on a line where everything collides (`Found::complete` false) |
| A corrupted or crafted stick byte stream crashes or stalls the radio receiver | `stick::Receiver` uses a fixed buffer, checks each frame's CRC or checksum, skips at least one byte per error and drops a full buffer without a frame; fuzzed |
| Bytes appended to a mode-10 telegram outside its tag pass as authenticated | The unencrypted tail is CCM associated data; a telegram whose tag does not cover it is refused |
| A stripped AFL MAC downgrades a mode-7 telegram to "decrypted" | A MAC announced in the message control but absent is `Check::Mac` |
| Meter-supplied text breaks the JSON a consumer parses | One RFC 8259 escaper for every string (manufacturer codes included); strict-parser tests and a fuzz target |
| A key leaks | `oms::Key` is zeroized on drop and prints as `Key(..)`; errors name the failed check only; the crate stores no key |
| A corrupted reading is used | Wired checksum and every wireless block CRC are checked |
| A VIFE changes a value and the change is ignored (a reading off by ×1000) | `Record::quantity` applies multiplicative corrections exactly and returns `None` for every VIFE it does not model |
| Meter-supplied text (LVAR strings, plain-text units) carries terminal escape sequences into a log or console | `Display` escapes everything but printable ASCII as `\xNN` |

## What it does not defend against

- **Forgery of unauthenticated telegrams.** wM-Bus is broadcast. Plaintext telegrams
  can be forged by anyone in range; mode-5 and ELL telegrams by anyone with the key,
  which is often shared across a fleet, and **modified without the key**: mode 5's IV
  comes from cleartext header fields and the ELL uses CTR with a linear CRC, so bits of
  the plaintext can be flipped undetected (both demonstrated in `tests/oms.rs`). The
  unencrypted tail of a partially encrypted payload has no protection at all
  (`Record::from_ciphertext` is false for it). Every telegram carries its `Authenticity`; only
  `Authenticated` (mode 7, CMAC verified) is evidence.
- **Replay.** An authenticated telegram stays authenticated when replayed. The crate is
  stateless: it returns the AFL message counter, and rejecting a counter that is not
  greater than the last accepted one is the caller's job.
- **Format-cache poisoning.** A compact frame's layout is found by a CRC-16 signature,
  not a MAC. A forged format frame or plaintext full frame can make a colliding layout
  re-read genuine data as other records. Fill the cache only from frames as trustworthy
  as the compact frames it serves.
- **A forged fragment 1** restarts reassembly; the master requests the message again.
- **Stick metadata.** A receiver stick's CRC check, RSSI and "decrypted by stick" flag
  prove nothing about the sender.
- **The `aes` crate's own code paths.** With `oms`, the software key schedule of `aes`
  keeps unreachable bounds checks, so a firmware binary links a panic path from it.
- Mode 13 (TLS) and the key exchange are not implemented.
