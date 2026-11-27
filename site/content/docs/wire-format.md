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

## The CI field and header (EN 13757-3)

| CI | Header | Then |
|---|---|---|
| `72` | long: ID (4, BCD), manufacturer (2), version, medium, access no., status, configuration (2) | records |
| `7A` | short: access no., status, configuration (2) | records |
| `78` | none | records |

The configuration field announces encryption: mode in the high byte (bits 0–4), the
number of encrypted 16-byte blocks in the high nibble of the low byte. OMS mode 5 with
two blocks is `20 05` on the wire.

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

## The primary VIF table

| VIF | Quantity | Unit and exponent |
|---|---|---|
| `00`–`07` | energy | Wh × 10^(n−3) |
| `08`–`0F` | energy | J × 10^n |
| `10`–`17` | volume | m³ × 10^(n−6) |
| `18`–`1F` | mass | kg × 10^(n−3) |
| `20`–`27` | on time, operating time | s, min, h, d |
| `28`–`2F` | power | W × 10^(n−3) |
| `38`–`3F` | volume flow | m³/h × 10^(n−6) |
| `58`–`5F` | flow / return temperature | °C × 10^(nn−3) |
| `60`–`63` | temperature difference | K × 10^(nn−3) |
| `68`–`6B` | pressure | bar × 10^(nn−3) |
| `6C` / `6D` | date (type G) / date and time (type F) | |
| `78` | fabrication number | |
| `7C` | plain-text unit | |
| `FB`, `FD` | extension tables (first VIFE) | |

## The reference telegram, decoded

```text
68 1F 1F 68  08 02 72                      RSP_UD from address 2, long header
78 56 34 12  24 40  01  07  55  00  00 00  ID 12345678, PAD, v1, water, access 0x55
03 13  15 31 00                            volume       12565 × 10⁻³ m³ = 12.565 m³
DA 02 3B  13 01                            volume flow  max, storage 5: 0.113 m³/h
8B 60 04  37 18 02                         energy       tariff 2, subunit 1: 218370 Wh
18 16                                      checksum, stop
```
