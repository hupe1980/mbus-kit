# Threat model

## What mbus-kit trusts

Nothing it decodes. Bytes come from a wired bus, a radio receiver anyone in range can
transmit to, or a capture file.

## What it defends against

| Threat | Defence |
|---|---|
| Malformed or crafted input crashes the process | No `unsafe`; panics, `unwrap` and indexing denied by lint; panic-symbol scan of the no_std release build; fuzz targets on wired, wireless and records |
| Input exhausts memory | No allocation; wireless frames are copied into a caller buffer and a frame that does not fit is an error |
| Input exhausts CPU | DIFE and VIFE chains are capped at 10; every loop consumes input |
| A corrupted reading is used | Wired checksum and every wireless block CRC are checked |

## What it does not defend against

- **Forgery and eavesdropping on the radio.** wM-Bus is broadcast. Unencrypted
  telegrams can be read and forged by anyone in range; a CRC is not authentication.
  OMS security modes 5 and 7 (AES) are **detected but not decrypted or verified** yet.
  Until they are, readings received by radio are not evidence of anything.
- **Replay.** The access number helps a caller detect repeats; this crate does not track
  state across telegrams.
