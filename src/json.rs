//! Telegrams as JSON, without allocating: `Display` adapters that write one object.
//!
//! ```
//! use mbus_kit::{json::Json, Application};
//!
//! let app = Application::parse(0x78, &[0x0C, 0x13, 0x27, 0x04, 0x85, 0x02])?;
//! assert_eq!(
//!     Json::new(&app).to_string(),
//!     r#"{"meter":null,"ci":"78","authenticity":"none","body":"records","records":[{"name":"volume","function":"instantaneous","storage":0,"tariff":0,"subunit":0,"modifiers":[],"value":2850.427,"unit":"m3","text":"volume — 2850.427 m³"}]}"#
//! );
//! # Ok::<(), mbus_kit::Error>(())
//! ```
//!
//! Schema (one object per telegram; fields in this order, absent ones left out):
//!
//! | Field | Type | Meaning |
//! |---|---|---|
//! | `meter` | object or `null` | `manufacturer` (3 letters), `id` (8 digits, or 8 hex digits when not BCD), `version`, `medium` (name), `medium_code` |
//! | `ci` | string | the CI field, two hex digits |
//! | `access`, `status` | number | from the transport header, if it has one |
//! | `rssi_dbm` | number | when given with [`Json::rssi`] |
//! | `authenticity` | string | `none`, `encrypted` (decrypted, not integrity-protected) or `authenticated` |
//! | `counter` | number | the message counter the MAC or tag covers (`authenticated` only) |
//! | `body` | string | `records`, `encrypted`, `compact`, `format`, `fragment`, `application_error`, `alarm`, `empty` |
//! | `security` | string | for `encrypted`: `mode5`, `mode7`, `mode10`, `ell`, `mode<n>` |
//! | `signature` | string | for `compact` and `format`: the format signature, four hex digits |
//! | `bytes` | string | for `application_error` and `alarm`: the payload in hex |
//! | `records` | array | for `records`: see below |
//! | `manufacturer_data` | string | bytes after a `0F`/`1F` DIF, in hex |
//!
//! A record is `name`, `function`, `storage`, `tariff`, `subunit`, `modifiers` (strings),
//! then `value` and `unit`: an exact decimal **number literal** with an ASCII unit code
//! (`m3`, `Wh`, `degC`, `m3/h`, …) when the record is a quantity; otherwise a string (dates
//! and times in ISO 8601, text, hex bytes) or `null` (no value, NaN, invalid BCD). Then
//! `encrypted` (`true` when the record came out of decryption) and `text`, the record's
//! one-line description. A compact profile has `"value":null` and a `profile` object:
//! `direction` (`forward`, `backward`, `registers`), `elements_are` (`readings`,
//! `increments`, `decrements`, `differences`), `spacing` (`{"every":15,"unit":"s"}`,
//! months as `"mo"`, or `"columns"`) and `elements`, exact numbers in the record's unit
//! (`null` for the "illegal" preset). A record that does not parse is
//! `{"error": "…"}`, and the list ends there. Strings are escaped per RFC 8259.

use core::fmt::{self, Write};

use crate::app::{Address, Application, Authenticity, Body, Security};
use crate::profile::{Increment, Profile, Spacing};
use crate::record::Record;
use crate::value::Value;
use crate::vif::Direction;

/// One telegram as a JSON object.
#[derive(Clone, Copy, Debug)]
pub struct Json<'x, 'a> {
    app: &'x Application<'a>,
    rssi_dbm: Option<i16>,
}

impl<'x, 'a> Json<'x, 'a> {
    /// The JSON of `app`.
    pub const fn new(app: &'x Application<'a>) -> Self {
        Self { app, rssi_dbm: None }
    }

    /// Include the signal strength a receiver reported.
    #[must_use]
    pub const fn rssi(mut self, dbm: Option<i16>) -> Self {
        self.rssi_dbm = dbm;
        self
    }
}

impl fmt::Display for Json<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let app = self.app;
        f.write_str("{\"meter\":")?;
        match app.meter() {
            Some(a) => meter(f, a)?,
            None => f.write_str("null")?,
        }
        write!(f, ",\"ci\":\"{:02X}\"", app.ci)?;
        if let Some(s) = app.header.short() {
            write!(f, ",\"access\":{},\"status\":{}", s.access_number, s.status)?;
        }
        if let Some(dbm) = self.rssi_dbm {
            write!(f, ",\"rssi_dbm\":{dbm}")?;
        }
        match app.authenticity {
            Authenticity::None => f.write_str(",\"authenticity\":\"none\"")?,
            Authenticity::Encrypted => f.write_str(",\"authenticity\":\"encrypted\"")?,
            Authenticity::Authenticated { counter } => {
                write!(f, ",\"authenticity\":\"authenticated\",\"counter\":{counter}")?;
            }
        }
        match &app.body {
            Body::Records(records) => {
                f.write_str(",\"body\":\"records\",\"records\":[")?;
                let mut records = records.clone();
                let mut first = true;
                for r in records.by_ref() {
                    if !first {
                        f.write_char(',')?;
                    }
                    first = false;
                    match r {
                        Ok(r) => JsonRecord(&r).fmt(f)?,
                        Err(e) => {
                            f.write_str("{\"error\":")?;
                            string(f, format_args!("{e}"))?;
                            f.write_char('}')?;
                        }
                    }
                }
                f.write_char(']')?;
                if let Some(m) = records.manufacturer_data().filter(|m| !m.is_empty()) {
                    f.write_str(",\"manufacturer_data\":\"")?;
                    hex(f, m)?;
                    f.write_char('"')?;
                }
            }
            Body::Encrypted(e) => {
                let security = match e.security {
                    Security::Mode5 => "mode5",
                    Security::Mode7 => "mode7",
                    Security::Mode10 => "mode10",
                    Security::EllCtr => "ell",
                    Security::Other(m) => {
                        return write!(f, ",\"body\":\"encrypted\",\"security\":\"mode{m}\"}}");
                    }
                };
                write!(f, ",\"body\":\"encrypted\",\"security\":\"{security}\"")?;
            }
            Body::Compact(c) => write!(f, ",\"body\":\"compact\",\"signature\":\"{:04X}\"", c.signature)?,
            Body::Format(c) => write!(f, ",\"body\":\"format\",\"signature\":\"{:04X}\"", c.signature)?,
            Body::Fragment(_) => f.write_str(",\"body\":\"fragment\"")?,
            Body::Empty => f.write_str(",\"body\":\"empty\"")?,
            Body::ApplicationError(b) | Body::Alarm(b) => {
                let kind = if matches!(app.body, Body::Alarm(_)) { "alarm" } else { "application_error" };
                write!(f, ",\"body\":\"{kind}\",\"bytes\":\"")?;
                hex(f, b)?;
                f.write_char('"')?;
            }
        }
        f.write_char('}')
    }
}

/// One record as a JSON object (the elements of `records` in [`Json`]).
#[derive(Clone, Copy, Debug)]
pub struct JsonRecord<'x, 'a>(pub &'x Record<'a>);

impl fmt::Display for JsonRecord<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = self.0;
        f.write_str("{\"name\":")?;
        string(f, format_args!("{}", r.vib.description().kind))?;
        f.write_str(",\"function\":")?;
        string(f, format_args!("{}", r.dib.function))?;
        write!(
            f,
            ",\"storage\":{},\"tariff\":{},\"subunit\":{}",
            r.dib.storage, r.dib.tariff, r.dib.subunit
        )?;
        f.write_str(",\"modifiers\":[")?;
        for (i, m) in r.vib.modifiers().enumerate() {
            if i > 0 {
                f.write_char(',')?;
            }
            string(f, format_args!("{m}"))?;
        }
        f.write_str("],\"value\":")?;
        if let Some(p) = r.profile() {
            return profile(f, r, &p);
        }
        match (r.quantity(), r.value) {
            (Some(q), _) => {
                // The exact decimal, without the unit (or its prefix).
                write!(f, "{}", crate::Quantity { unit: None, ..q })?;
                if let Some(unit) = q.unit {
                    f.write_str(",\"unit\":\"")?;
                    ascii_unit(f, unit.symbol())?;
                    f.write_char('"')?;
                } else if let Some(text) = r.vib.plain_text() {
                    f.write_str(",\"unit\":")?;
                    string(f, format_args!("{}", Reversed(text)))?;
                }
            }
            (None, v) => value(f, &v)?,
        }
        if r.from_ciphertext {
            f.write_str(",\"encrypted\":true")?;
        }
        f.write_str(",\"text\":")?;
        string(f, format_args!("{r}"))?;
        f.write_char('}')
    }
}

/// A meter address as the JSON object used for `meter`.
#[derive(Clone, Copy, Debug)]
pub struct JsonAddress<'x>(pub &'x Address);

impl fmt::Display for JsonAddress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        meter(f, *self.0)
    }
}

/// A compact profile: `"value":null`, the unit, and `profile` with the elements as exact
/// numbers in the record's unit (`null` for the "illegal" preset).
fn profile(f: &mut fmt::Formatter<'_>, r: &Record<'_>, p: &Profile<'_>) -> fmt::Result {
    let d = r.vib.description();
    f.write_str("null")?;
    if let Some(unit) = d.unit {
        f.write_str(",\"unit\":\"")?;
        ascii_unit(f, unit.symbol())?;
        f.write_char('"')?;
    }
    let direction = match p.direction {
        Direction::Forward => "forward",
        Direction::Inverse => "backward",
        Direction::WithRegisters => "registers",
    };
    let increment = match p.increment {
        Increment::Absolute => "readings",
        Increment::Positive => "increments",
        Increment::Negative => "decrements",
        Increment::Difference => "differences",
    };
    write!(f, ",\"profile\":{{\"direction\":\"{direction}\",\"elements_are\":\"{increment}\",\"spacing\":")?;
    match p.spacing {
        Spacing::Every(n, unit) => {
            write!(f, "{{\"every\":{n},\"unit\":\"")?;
            ascii_unit(f, unit.symbol())?;
            f.write_str("\"}")?;
        }
        Spacing::HalfMonth => f.write_str("{\"every\":0.5,\"unit\":\"mo\"}")?,
        Spacing::Months(n) => write!(f, "{{\"every\":{n},\"unit\":\"mo\"}}")?,
        Spacing::Columns => f.write_str("\"columns\"")?,
        Spacing::Reserved(v) => write!(f, "{{\"reserved\":{v}}}")?,
    }
    f.write_str(",\"elements\":[")?;
    for k in 0..p.len() {
        if k > 0 {
            f.write_char(',')?;
        }
        match p.element(k) {
            Some(e) => write!(f, "{}", crate::Quantity { mantissa: e, exponent: d.exponent, unit: None })?,
            None => f.write_str("null")?,
        }
    }
    f.write_str("]}")?;
    if r.from_ciphertext {
        f.write_str(",\"encrypted\":true")?;
    }
    f.write_str(",\"text\":")?;
    string(f, format_args!("{r}"))?;
    f.write_char('}')
}

/// A string as a JSON string literal, quotes included.
#[derive(Clone, Copy, Debug)]
pub struct JsonString<'s>(pub &'s str);

impl fmt::Display for JsonString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        string(f, format_args!("{}", self.0))
    }
}

fn meter(f: &mut fmt::Formatter<'_>, a: Address) -> fmt::Result {
    // A crafted address decodes to any of `@`, `[`, `\`, … : escape it too.
    f.write_str("{\"manufacturer\":")?;
    string(f, format_args!("{}", a.manufacturer))?;
    f.write_str(",\"id\":\"")?;
    if let Some(n) = a.id_number() {
        write!(f, "{n:08}")?;
    } else {
        let [x, y, z, w] = a.id;
        write!(f, "{w:02X}{z:02X}{y:02X}{x:02X}")?;
    }

    write!(f, "\",\"version\":{},\"medium\":", a.version)?;
    string(f, format_args!("{}", a.medium))?;
    write!(f, ",\"medium_code\":{}}}", a.medium.0)
}

/// A value that is not a quantity: integers as numbers, everything else as a string.
fn value(f: &mut fmt::Formatter<'_>, v: &Value<'_>) -> fmt::Result {
    match *v {
        Value::Int(n) | Value::Bcd(n) => write!(f, "{n}"),
        Value::UInt(n) => write!(f, "{n}"),
        Value::None | Value::InvalidBcd(_) | Value::Real32(_) => f.write_str("null"),
        Value::Date(d) => write!(f, "\"{d}\""),
        Value::Time(t) => write!(f, "\"{t}\""),
        Value::DateTime(t) => {
            let (d, tm) = (t.date, t.time);
            write!(f, "\"{:04}-{:02}-{:02}T{:02}:{:02}", d.year, d.month, d.day, tm.hour, tm.minute)?;
            if t.has_seconds {
                write!(f, ":{:02}", tm.second)?;
            }
            f.write_char('"')
        }
        Value::Text(_) | Value::Bytes(_) | Value::Duration(_) => string(f, format_args!("{v}")),
    }
}

/// A JSON string: quotes, backslashes and control characters escaped (RFC 8259 §7).
fn string(f: &mut fmt::Formatter<'_>, args: fmt::Arguments<'_>) -> fmt::Result {
    f.write_char('"')?;
    Escape(f).write_fmt(args)?;
    f.write_char('"')
}

struct Escape<'f, 'g>(&'f mut fmt::Formatter<'g>);

impl Write for Escape<'_, '_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            match c {
                '"' => self.0.write_str("\\\"")?,
                '\\' => self.0.write_str("\\\\")?,
                c if (c as u32) < 0x20 => write!(self.0, "\\u{:04x}", c as u32)?,
                c => self.0.write_char(c)?,
            }
        }
        Ok(())
    }
}

/// `m³` → `m3`, `°C` → `degC`, `µg/m³` → `ug/m3`: unit codes a program can match on.
fn ascii_unit(f: &mut fmt::Formatter<'_>, symbol: &str) -> fmt::Result {
    for c in symbol.chars() {
        match c {
            '³' => f.write_char('3')?,
            '²' => f.write_char('2')?,
            'µ' => f.write_char('u')?,
            '°' => f.write_str("deg")?,
            c if c.is_ascii() => f.write_char(c)?,
            _ => f.write_char('?')?,
        }
    }
    Ok(())
}

fn hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    bytes.iter().try_for_each(|b| write!(f, "{b:02X}"))
}

/// A plain-text unit, stored reversed on the wire, in reading order with `\xNN` escapes.
struct Reversed<'a>(&'a [u8]);

impl fmt::Display for Reversed<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::value::escaped(f, self.0.iter().rev())
    }
}
