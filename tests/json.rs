//! JSON output (spec 006 US1): every corpus telegram renders to JSON that a strict RFC 8259
//! parser accepts, with one record object per record and the exact quantity as its number.

use std::fs;
use std::path::Path;

use mbus_kit::json::Json;
use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body, Quantity, Records};

/// A parsed JSON value (numbers kept as their literal text).
#[derive(Debug, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// A strict parser: RFC 8259 grammar, no trailing data, no leading zeros, valid escapes.
struct P<'a>(&'a [u8], usize);

impl P<'_> {
    fn peek(&self) -> Option<u8> {
        self.0.get(self.1).copied()
    }
    fn eat(&mut self, b: u8) -> Result<(), String> {
        if self.peek() == Some(b) {
            self.1 += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at {}", b as char, self.1))
        }
    }
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.1 += 1;
        }
    }
    fn value(&mut self) -> Result<J, String> {
        self.ws();
        let v = match self.peek().ok_or("end")? {
            b'{' => {
                self.1 += 1;
                let mut kv = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.1 += 1;
                    return Ok(J::Obj(kv));
                }
                loop {
                    self.ws();
                    let J::Str(k) = self.string()? else { unreachable!() };
                    self.ws();
                    self.eat(b':')?;
                    let v = self.value()?;
                    assert!(kv.iter().all(|(x, _)| *x != k), "duplicate key {k}");
                    kv.push((k, v));
                    self.ws();
                    if self.peek() == Some(b',') {
                        self.1 += 1;
                        continue;
                    }
                    self.eat(b'}')?;
                    break J::Obj(kv);
                }
            }
            b'[' => {
                self.1 += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.1 += 1;
                    return Ok(J::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    self.ws();
                    if self.peek() == Some(b',') {
                        self.1 += 1;
                        continue;
                    }
                    self.eat(b']')?;
                    break J::Arr(a);
                }
            }
            b'"' => self.string()?,
            b't' => self.word("true", J::Bool(true))?,
            b'f' => self.word("false", J::Bool(false))?,
            b'n' => self.word("null", J::Null)?,
            _ => self.number()?,
        };
        Ok(v)
    }
    fn word(&mut self, w: &str, v: J) -> Result<J, String> {
        if self.0[self.1..].starts_with(w.as_bytes()) {
            self.1 += w.len();
            Ok(v)
        } else {
            Err(format!("bad literal at {}", self.1))
        }
    }
    fn string(&mut self) -> Result<J, String> {
        self.eat(b'"')?;
        let start = self.1;
        loop {
            match self.peek().ok_or("unterminated string")? {
                b'"' => break,
                b'\\' => {
                    self.1 += 1;
                    match self.peek().ok_or("bad escape")? {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.1 += 1,
                        b'u' => {
                            let h = self.0.get(self.1 + 1..self.1 + 5).ok_or("short \\u")?;
                            assert!(h.iter().all(u8::is_ascii_hexdigit), "bad \\u");
                            self.1 += 5;
                        }
                        _ => return Err(format!("bad escape at {}", self.1)),
                    }
                }
                c if c < 0x20 => return Err(format!("raw control byte at {}", self.1)),
                _ => self.1 += 1,
            }
        }
        let s = std::str::from_utf8(&self.0[start..self.1]).map_err(|e| e.to_string())?.to_owned();
        self.1 += 1;
        Ok(J::Str(s))
    }
    fn number(&mut self) -> Result<J, String> {
        let start = self.1;
        if self.peek() == Some(b'-') {
            self.1 += 1;
        }
        match self.peek() {
            Some(b'0') => self.1 += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.1 += 1;
                }
            }
            _ => return Err(format!("bad number at {start}")),
        }
        if self.peek() == Some(b'.') {
            self.1 += 1;
            let d = self.1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.1 += 1;
            }
            if d == self.1 {
                return Err(format!("bad fraction at {start}"));
            }
        }
        Ok(J::Num(std::str::from_utf8(&self.0[start..self.1]).unwrap().to_owned()))
    }
}

fn parse(text: &str) -> J {
    let mut p = P(text.as_bytes(), 0);
    let v = p.value().unwrap_or_else(|e| panic!("{e}: {text}"));
    p.ws();
    assert_eq!(p.1, text.len(), "trailing data: {text}");
    v
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(char::is_ascii_hexdigit).collect();
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

/// Check one application's JSON against its records.
fn check(app: &Application<'_>, name: &str) -> usize {
    let text = Json::new(app).rssi(Some(-70)).to_string();
    assert!(!text.contains('\n'), "{name}: one line");
    let j = parse(&text);
    assert_eq!(j.get("rssi_dbm"), Some(&J::Num("-70".into())));
    let Body::Records(records) = &app.body else { return 0 };
    let Some(J::Arr(items)) = j.get("records") else { panic!("{name}: no records array") };
    let records: Vec<_> = records.clone().collect();
    assert_eq!(items.len(), records.len(), "{name}");
    for (item, r) in items.iter().zip(&records) {
        let Ok(r) = r else {
            assert!(item.get("error").is_some(), "{name}");
            continue;
        };
        let value = item.get("value").unwrap_or_else(|| panic!("{name}: no value"));
        if let Some(q) = r.quantity() {
            // 006-json-and-cli/SR-002 (SC-001): the number is the exact quantity.
            assert_eq!(value, &J::Num(Quantity { unit: None, ..q }.to_string()), "{name}: {r}");
        }
        assert_eq!(item.get("text"), Some(&J::Str(json_escaped(&r.to_string()))), "{name}");
    }
    records.len()
}

/// What `text` looks like inside a JSON string (only `"` and `\` matter for corpus text).
fn json_escaped(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[test]
fn every_corpus_telegram_renders_to_strict_json_with_exact_values() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut checked = 0;
    for dir in ["wired", "wireless"] {
        for e in fs::read_dir(root.join(dir)).unwrap() {
            let path = e.unwrap().path();
            let text = fs::read_to_string(&path).unwrap();
            let format = text.lines().find_map(|l| l.strip_prefix("format ")).map(|f| match f.trim() {
                "a" => Format::A,
                "b" => Format::B,
                _ => Format::Stripped,
            });
            let frame = text
                .lines()
                .filter(|l| {
                    !l.is_empty()
                        && !l.starts_with(['#', '!', '='])
                        && !l.starts_with("format")
                        && !l.starts_with("key")
                })
                .map(hex)
                .next()
                .unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut buf = [0u8; 512];
            let app = match format {
                None => match Frame::parse(&frame).unwrap().unwrap().0 {
                    Frame::Long { ci, data, .. } => Application::parse(ci, data),
                    _ => continue,
                },
                Some(f) => wireless::decode(&frame, f, &mut buf).unwrap().unwrap().0.application(),
            };
            let Ok(app) = app else { continue };
            #[cfg(feature = "oms")]
            if let (Some(key), Body::Encrypted(e)) =
                (text.lines().find_map(|l| l.strip_prefix("key ")), &app.body)
            {
                let mut plain = [0u8; 512];
                let key = mbus_kit::oms::Key::from_hex(key.trim()).unwrap();
                let open = e.decrypt(&key, &mut plain).unwrap();
                checked += check(&open, &name);
                assert!(Json::new(&open).to_string().contains("\"authenticity\":\""));
                continue;
            }
            checked += check(&app, &name);
        }
    }
    assert!(checked > 300, "{checked} records");
}

#[test]
fn hostile_meter_text_stays_valid_json() {
    // 006-json-and-cli/SR-001 (SC-002): quotes, backslashes, control bytes and non-ASCII in an LVAR string
    // and a plain-text unit.
    let bytes = hex("0D FD 11 06 22 5C 0A 00 FF 41  0C 7C 03 22 0A 5C 01 00 00 00");
    let app = Application::parse(0x78, &bytes).unwrap();
    let text = Json::new(&app).to_string();
    let j = parse(&text);
    let Some(J::Arr(items)) = j.get("records") else { panic!() };
    assert_eq!(items.len(), 2, "{text}");
    assert!(items.iter().all(|i| i.get("error").is_none()), "{text}");
    // The plain-text unit, in reading order, is the unit string.
    assert_eq!(items[1].get("unit"), Some(&J::Str(r#"\\x5c\\x0a\""#.into())), "{text}");
}

#[test]
fn bodies_without_records_say_what_they_are() {
    let bytes = hex("01 00 10 05 0102030405060708090A0B0C0D0E0F10");
    let enc = Application::parse(0x7A, &bytes).unwrap();
    let j = parse(&Json::new(&enc).to_string());
    assert_eq!(j.get("body"), Some(&J::Str("encrypted".into())));
    assert_eq!(j.get("security"), Some(&J::Str("mode5".into())));
    assert_eq!(j.get("access"), Some(&J::Num("1".into())));
    let err = Application::parse(0x70, &[0x01]).unwrap();
    let j = parse(&Json::new(&err).to_string());
    assert_eq!(
        (j.get("body"), j.get("bytes")),
        (Some(&J::Str("application_error".into())), Some(&J::Str("01".into())))
    );
}

#[test]
fn every_value_kind_has_its_json_type() {
    // Date, date-time, text, invalid BCD, NaN, an unsigned integer.
    let bytes = hex("02 6C 1F 15  04 6D 32 37 1F 15  0D FD 11 02 41 42  0A 13 1A 00  05 2B 00 00 C0 7F");
    let app = Application::parse(0x78, &bytes).unwrap();
    let j = parse(&Json::new(&app).to_string());
    let Some(J::Arr(items)) = j.get("records") else { panic!() };
    let values: Vec<&J> = items.iter().map(|i| i.get("value").unwrap()).collect();
    assert_eq!(values[0], &J::Str("2008-05-31".into()));
    assert_eq!(values[1], &J::Str("2008-05-31T23:50".into()));
    assert_eq!(values[2], &J::Str("BA".into()), "text in reading order");
    assert_eq!(values[3], &J::Null, "invalid BCD is not a number");
    assert_eq!(values[4], &J::Null, "NaN is not a number");
    let _ = Records::new(&[]);
}

#[test]
fn a_crafted_manufacturer_code_is_escaped() {
    // Found by the json fuzz target: 0x1C0... decodes to letters outside A–Z, e.g. `\`.
    let mut h = hex("78563412 00 00 01 00 00 00 00 00");
    h[4..6].copy_from_slice(&0x7180u16.to_le_bytes()); // "\@@"-like letters
    let app = Application::parse(0x72, &h).unwrap();
    let text = Json::new(&app).to_string();
    parse(&text);
}
