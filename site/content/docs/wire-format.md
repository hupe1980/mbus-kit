+++
title = "The wire format"
description = "Wired and wireless M-Bus frames, the CI field, data records and the VIF table, byte by byte."
weight = 20
+++

## Wired frames (EN 13757-2)

```text
E5                                  ACK
10  C  A  CS  16                    short frame       CS = C + A
68  L  L  68  C  A  CI  …  CS  16   long frame        CS = sum of the L bytes from C
```

| C | Meaning |
|---|---|
| `40` | `SND_NKE`: reset the slave |
| `53` / `73` | `SND_UD`: send data to the slave (FCB 0/1) |
| `5B` / `7B` | `REQ_UD2`: request the reading (FCB 0/1) |
| `08`, `18`, `28`, `38` | `RSP_UD`: the slave's response |

## Wireless frames (EN 13757-4)

```text
L  C  M M  A A A A A A  CI …
      ^^^  ID (BCD, LSB first), version, device type
      manufacturer (three letters in 15 bits, LE)
```

**Format A:** a CRC after the first 10 bytes and after every following block of 16.
**Format B:** one CRC after block 2 (covering blocks 1 and 2), one after block 3. The
CRC is CRC-16/EN-13757 (polynomial `3D65`), most significant byte first.

## The layers above the link layer

```text
[ELL 8C–8F]  [AFL 90]  TPL CI + header  records
```

| CI | Layer | Content |
|---|---|---|
| `8C` / `8E` | extended link layer | communication control, access number (`8E`: receiver address) |
| `8D` / `8F` | the same, encrypted | + session number, payload CRC; AES-CTR when the session number says so |
| `90` | authentication and fragmentation | fragment control, message control, key info, message counter, CMAC, message length |
| `72` / `7A` / `78` | transport layer | long header, short header, none; then records |
| `73` / `7B` / `79` | compact frame | the same headers; then format signature, full-frame CRC, data only |
| `6B` / `6A` / `69` | format frame | the same headers; then a record layout and its signature |

Each layer appears at most once, in this order. See [Security](@/docs/security.md).

### Fragments

A message too long for one frame is split into fragments, each a frame of its own with
an AFL. Fragment control: bit 14 *more fragments*, bits 13–9 which fields follow, bits
7–0 the fragment ID (0 = not fragmented, then 1, 2, 3, …). The first fragment carries
message control, counter and the message length, and the transport layer starts in it;
pieces are cut anywhere, even mid-record. The last carries the MAC over the whole
message.

OMS Annex N N.3, link-layer CRCs removed:

```text
fragment 1:  … 90 09 01 78 65 B3 0A 00 00 56 00 | 7A 05 00 50 07 10 30 53 …   (ID 1, more; MCL, counter, ML 86)
fragment 2:  … 90 02 02 40                       | C4 90 27 8E …               (ID 2, more)
fragment 3:  … 90 0A 03 04 BE 47 ED 4C 9C C1 1A 78 | 06 CC 3E …               (ID 3, MAC)
```

### Compact frames

A meter sends a full frame now and then and compact frames in between: CI `79`, then the
**format signature** (CRC-16 of every record's DIF/DIFE/VIF/VIFE bytes), the **full-frame
CRC** (CRC-16 of the records rebuilt), and the data bytes only. The receiver keeps the
layouts it has seen; `Compact::expand` rebuilds the records and checks the full-frame CRC.

## The CI field and header (EN 13757-3)

| CI | Header | Then |
|---|---|---|
| `72` | long: ID (4, BCD), manufacturer (2), version, medium, access no., status, configuration (2) | records |
| `7A` | short: access no., status, configuration (2) | records |
| `78` | none | records |

The configuration field announces encryption: mode in the high byte (bits 0–4), the
number of encrypted 16-byte blocks in the high nibble of the low byte (0: none, 15: all).
OMS mode 5 with two blocks is `20 05` on the wire; mode 7 adds a configuration field
extension byte (key derivation, key id). Mode 10 counts encrypted *bytes* in the low byte
(`FF`: all) and adds a 2-byte extension, a key version, the message counter, and a tag at
the end.

## A data record

```text
DIF  [DIFE…]  VIF  [VIFE…]  data
```

| DIF bits | Meaning |
|---|---|
| 7 | another DIFE follows |
| 6 | storage number, least significant bit |
| 5–4 | function: instantaneous, maximum, minimum, during error |
| 3–0 | data: `1`–`4`, `6`, `7` integer of 1–4, 6, 8 bytes · `5` real · `9`–`C`, `E` BCD of 1–4, 6 bytes · `D` variable (LVAR) |

Each DIFE adds 4 storage bits, 2 tariff bits and 1 subunit bit. `2F` is filler; `0F`
and `1F` start manufacturer-specific data.

## The value information block

```text
VIF  [VIFE …]          bit 7 of each byte: another VIFE follows (at most 10)
```

The **VIF** picks a quantity, unit and decimal exponent from the primary table. Two
codes point to an extension table whose code is the **first VIFE**: `FB` (larger units)
and `FD` (identifiers, versions, error flags, volts, amperes; `FD FD` for OMS sensors).
Every further VIFE is **combinable**: it modifies the value.

| VIF | Quantity | Unit and exponent |
|---|---|---|
| `00`–`07` | energy | Wh × 10^(n−3) |
| `08`–`0F` | energy | J × 10^n |
| `10`–`17` | volume | m³ × 10^(n−6) |
| `18`–`1F` | mass | kg × 10^(n−3) |
| `20`–`27` | on time, operating time | s, min, h, d |
| `28`–`2F` | power | W × 10^(n−3) |
| `38`–`4F` | volume flow | m³/h, m³/min, m³/s |
| `58`–`67` | flow, return, difference, external temperature | °C / K × 10^(nn−3) |
| `68`–`6B` | pressure | bar × 10^(nn−3) |
| `6C` / `6D` | date (type G) / time (J), date and time (F, I) by size | |
| `78`, `79`, `7A` | fabrication number, identification, bus address | |
| `7C` | plain-text unit (length, then ASCII, last character first) | |
| `FB xx`, `FD xx` | extension tables | e.g. `FB 00` MWh × 10⁻¹, `FD 17` error flags |
| `7F` / `FF` | manufacturer specific | |

| Combinable VIFE | Meaning | `quantity()` |
|---|---|---|
| `70`–`77`, `7D` | × 10^(nnn−6), × 10³ | **applied** |
| `3A`, `3B`, `3C`, `40`/`48`, `68`/`6C`, `7E` | uncorrected, forward, backward, limits, during limit exceed, future value | kept |
| `20`–`35` | per second … per year, per litre, per kWh … | `None` |
| `39`, `42`–`4F`, `6A`–`6F` | start date of, date of a limit exceed | `None`, value is a date |
| `41`/`49`, `50`–`67` | number of exceeds, durations | `None` |
| `78`–`7B` | additive correction constant (offset) | `None` |
| `00`–`1C` | record error (no data, overflow, …) | `None` |
| `FC xx` | phases L1–L3, quadrants, unsigned (data type C) … | kept |
| `7F` | the rest is manufacturer specific | `None` |

`Record::quantity()` never returns a value a VIFE has changed in a way the crate does not
model: a missing reading is visible, a wrong one is not. `record.vib.modifiers()` lists
every combinable VIFE as a typed `Modifier`.

## Dates and times

| Type | Bytes | Content |
|---|---|---|
| G | 2 | date |
| J | 3 | time of day with seconds |
| F | 4 | date, hour, minute, invalid and summer-time flags |
| I | 6 | type F with seconds, day of week and week number |
| M | LVAR `E2`–`EA` | a time offset: count, then resolution (2 s, 1 s, 1/256 s, 1/32768 s); the relative form is a time before reception (`3 s before reception`) |

### Compact profiles

VIFE `1F` (forward), `13` (inverse) or `1E` (with registers) turn an LVAR record into a
series: spacing control (increments or readings, unit, element coding), spacing value
(seconds to days, half months, 1/3/6 months), then the elements. The series hangs off a
base value and a base time of the same storage number; `Profile::values(base)` yields
the readings, `Profile::offset(k)` and `Profile::date(k, base)` when each was taken.

A date the meter flags invalid, or one that cannot exist, is still returned:
`Date::is_valid()` and `DateTime::is_valid()` say so.

## The reference telegram, decoded

```text
68 1F 1F 68  08 02 72                      RSP_UD from address 2, long header
78 56 34 12  24 40  01  07  55  00  00 00  ID 12345678, PAD, v1, water, access 0x55
03 13  15 31 00                            volume       12565 × 10⁻³ m³ = 12.565 m³
DA 02 3B  13 01                            volume flow  max, storage 5: 0.113 m³/h
8B 60 04  37 18 02                         energy       tariff 2, subunit 1: 218370 Wh
18 16                                      checksum, stop
```
