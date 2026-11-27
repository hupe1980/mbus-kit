+++
title = "JSON and the mbus tool"
description = "Every telegram as one JSON line, exact values with units, and one command to decode captures, read wired meters and listen to radio sticks."
weight = 18
+++

## The tool

```sh
brew install hupe1980/tap/mbus                # macOS and Linux
cargo install mbus-kit --features cli         # anywhere with Rust; Windows: the release zip
mbus decode captures.hex --key 0102…0F11      # hex telegrams, wired or wireless, detected
mbus read /dev/ttyUSB0 5                      # a wired meter through a level converter
mbus read tcp:192.168.1.50:10001 12345678     # …or an Ethernet gateway, by secondary address
mbus search /dev/ttyUSB0                      # every meter on the bus
mbus listen /dev/ttyUSB0 --stick im871a --mode t1 --key 0102…0F11
```

Add `--json` to any command for one JSON object per telegram (JSON Lines). `mbus`
configures serial devices with `stty` (8E1 for M-Bus, 8N1 for sticks, 100 ms read
timeout); `--no-stty` leaves the port as it is. `decode` reads files or standard input,
one telegram per line; it accepts `key …` and `format a|b|stripped` lines, rtl-wmbus
lines, joins AFL fragments on consecutive lines and expands compact frames with layouts
learnt from earlier full frames.

```sh
mbus listen /dev/ttyUSB0 --stick iu891a --mode c1t1 --key … --json | mosquitto_pub -l -t meters
mbus decode archive.hex --json | jq 'select(.authenticity == "authenticated") | .records[0].value'
```

## One telegram

```json
{"meter":{"manufacturer":"ELS","id":"12345678","version":51,"medium":"gas","medium_code":3},
 "ci":"7A","access":117,"status":0,"authenticity":"authenticated","counter":2739,
 "body":"records","records":[
  {"name":"volume","function":"instantaneous","storage":0,"tariff":0,"subunit":0,
   "modifiers":[],"value":28504.27,"unit":"m3","encrypted":true,"text":"volume — 28504.27 m³"},
  {"name":"date and time","function":"instantaneous","storage":0,"tariff":0,"subunit":0,
   "modifiers":[],"value":"2008-05-31T23:50","encrypted":true,"text":"date and time — 2008-05-31 23:50"}]}
```

(wrapped here; the output is one line.)

| Field | Meaning |
|---|---|
| `meter` | manufacturer (3 letters), `id` (8 digits; 8 hex digits when the ID is not BCD), `version`, `medium` and `medium_code`; `null` if unknown |
| `ci`, `access`, `status` | the transport layer |
| `rssi_dbm` | from a receiver stick |
| `authenticity` | `none`: anyone could have sent it · `encrypted`: decrypted, **not** integrity-protected · `authenticated`: CMAC or CCM tag verified, with `counter` (compare it per meter to catch replays) |
| `body` | `records`, `encrypted` (+ `security`), `compact`/`format` (+ `signature`), `fragment`, `application_error`/`alarm` (+ `bytes`), `empty` |
| `records[]` | `name`, `function`, `storage`, `tariff`, `subunit`, `modifiers`, `value`, `unit`, `encrypted` (came out of decryption), `text` |

**Values.** A quantity is an exact decimal number literal (`28504.27`, never rounded
through a float) with an ASCII unit code: `Wh`, `J`, `W`, `m3`, `m3/h`, `kg`, `degC`, `K`,
`bar`, `s`, `h`, `V`, `A`, `Hz`, … Integers without a unit are numbers. Dates and times
are ISO 8601 strings, text is a string, binary data a hex string; NaN, an invalid BCD
value or a missing value is `null`. Parsing into a binary float on your side loses what a
float loses; `text` keeps the exact display.

**Profiles.** A compact profile record has `"value": null` and

```json
"profile":{"direction":"backward","elements_are":"increments",
           "spacing":{"every":8,"unit":"s"},"elements":[1,3,5,7,9,…]}
```

with the elements in the record's unit (`null` for the meter's "illegal" preset). Months
are `"unit":"mo"`. Turn them into readings from the base record with
`Profile::values` in code, or by summing in your consumer.

## In your own code

```rust
use mbus_kit::json::Json;

println!("{}", Json::new(&app).rssi(t.rssi_dbm));   // one line, no allocation
```

`Json` is a `Display` adapter in the `no_std` core: write it into a `String`, a socket, or
a fixed buffer on a microcontroller (`core::fmt::Write`). `JsonRecord`, `JsonAddress` and
`JsonString` render the parts. Every string is escaped per RFC 8259; meter-supplied bytes
cannot break out of the JSON (fuzzed with a strict validator).

## Verifying a download

Every release archive has a build provenance attestation, and `SHA256SUMS` lists their
checksums:

```sh
gh attestation verify mbus-0.1.0-x86_64-unknown-linux-gnu.tar.gz --repo hupe1980/mbus-kit
sha256sum --check --ignore-missing SHA256SUMS
```
