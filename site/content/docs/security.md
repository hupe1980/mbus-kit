+++
title = "Security"
description = "OMS security modes 5, 7 and 10 and the ELL, what each proves, how keys are handled, and what the caller must do about replays."
weight = 25
+++

## What a telegram can prove

Wireless M-Bus is broadcast. Anyone within range can receive, record, replay and send
telegrams; a capture file is no better. A CRC detects noise, not forgery. Every
`Application` therefore says what was verified:

| `Authenticity` | When | What it proves |
|---|---|---|
| `None` | plaintext, or no key | nothing: anyone could have sent it |
| `Encrypted` | mode 5, ELL AES-CTR, or mode 7 without an AFL MAC, decrypted with the meter's key | confidentiality only. **Not integrity:** without the key, an attacker can flip plaintext bits through the header fields that form the mode-5 IV, or through the CTR ciphertext with a recomputed CRC, and the checks still pass. And a fleet-wide key lets anyone who holds it send anything |
| `Authenticated { counter }` | mode 7 with an AFL MAC, or mode 10 with its CCM tag, verified | the message was made with the meter's key for this counter and is unmodified |

Only `Authenticated` is evidence of a reading. Treat the others as telemetry.

Mode 5 may encrypt only the first blocks; the records after them travel in the clear,
unprotected. `Record::from_ciphertext` tells them apart.

An AFL MAC is verified only for mode 7. When the message control announces a MAC that is
not there (stripped), the telegram is refused (`Check::Mac`), not downgraded. In front of a plaintext or mode-5 transport
layer it is parsed but not checked, and the telegram is not `Authenticated`.

## Replays are yours to catch

A recorded, authenticated telegram stays authenticated when it is sent again. The
library keeps no state, so it cannot tell. Keep the last accepted `counter` per meter and
reject any telegram whose counter is not greater. For mode 5 the TPL access number
(`header.short().access_number`) is the only hint, and it wraps at 255.

## The modes

| Layer | Encryption | Key | Check |
|---|---|---|---|
| TPL mode 5 | AES-128-CBC, IV = manufacturer ‖ address ‖ 8 × access number | the meter's | plaintext starts `2F 2F` |
| TPL mode 7 | AES-128-CBC, IV = 0 | derived per message: `CMAC(K, 00 ‖ counter ‖ ID ‖ 07…)` | AFL CMAC (8 bytes) over the transport layer, **verified before decryption**, then `2F 2F` |
| TPL mode 10 | AES-128-CCM, nonce = manufacturer ‖ address ‖ 00 ‖ counter | derived per message from the TPL counter | CCM tag (4–16 bytes) over header, data and any unencrypted tail, **verified before any record exists**; a tail the tag does not cover is refused; on failure the plaintext is erased |
| ELL `8D`/`8F` | AES-128-CTR, IV = manufacturer ‖ address ‖ CC ‖ session number ‖ 0 | the meter's | payload CRC |

The IV and the key derivation use the meter's address: the long header's when there is
one (a meter behind a radio adapter), the link layer's otherwise. The access number in
the mode-5 IV is always the transport layer's, even behind an ELL.

## Keys

- `oms::Key` holds 16 bytes and erases them when dropped (`zeroize`). Its `Debug` is
  `Key(..)`. The keys derived for mode 7 are `Key`s too and are erased when `decrypt`
  returns.
- The library never stores a key. It asks your `Keys` implementation for the meter's
  key during the call, and nothing else.
- A failed check says which check failed (`Check::Marker`, `Mac`, `Tag`, `PayloadCrc`,
  `Length`, `MissingCounter`), never anything about the key. A wrong key and a tampered
  telegram look the same, on purpose.

## Fragments

A long message arrives in AFL fragments; `fragment::Reassembly` joins them. Only the last
fragment carries the MAC, and it covers the whole message: message control, counter,
length and every byte of every fragment. A reassembled mode-7 message is `Authenticated`
only when that MAC verifies. A forged or replaced fragment fails it (tested bit by bit
against OMS Annex N N.3).

Without a MAC (mode 5, mode-7 without AFL MAC, plaintext) fragments are as forgeable as
any other telegram of that kind. And anyone in range can send a forged fragment 1, which
restarts the message: reassembly reports wrong, repeated and foreign fragments and keeps
the message in progress, but the master has to request a message again after a restart.

## Compact frames

A compact frame carries values without their DIF/VIF layout; the layout comes from your
cache, keyed by a CRC-16 signature. **A CRC is not a MAC.** Anyone in range can send a
format frame or a plaintext full frame whose layout collides with a genuine one and reads
the same data bytes as other records, with a matching full-frame CRC. Expanded records are
only as trustworthy as the frame their layout came from: fill the cache for a decrypted or
authenticated meter only from that meter's decrypted full frames.

## Receivers and converters

A wM-Bus stick checks the radio CRCs and reports an RSSI; neither says who sent the
telegram. A stick telegram is a radio telegram, with the authenticity the crate's own
checks give it ([Radio receivers](@/docs/receivers.md)). On the wire, `serial::Bus` bounds
every reply in time (first byte, quiet line, a hard end for the longest frame) and size
(512 bytes), and a search gives up after `Search::max_selections` (20 000 by default)
and says so (`Found::complete`): a stuck or babbling device cannot hold the master.

## Not covered

Mode 13 (TLS), the bidirectional key exchange, and compact frames inside TPL mode 5 or 7
(refused with `UnsupportedCi`: no public vector settles their CRC). Mode-13 telegrams
parse, with the body still `Encrypted`.
