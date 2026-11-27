//! Arbitrary telegrams rendered as JSON (006-json-and-cli/SR-001): every output must be one
//! line that a strict RFC 8259 validator accepts.
#![no_main]

use libfuzzer_sys::fuzz_target;
use mbus_kit::json::Json;
use mbus_kit::Application;

/// Validate one JSON value starting at `i`; returns the index after it.
fn value(b: &[u8], mut i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'{' | b'[' => {
            let close = if b[i] == b'{' { b'}' } else { b']' };
            let object = close == b'}';
            i += 1;
            if b.get(i) == Some(&close) {
                return Some(i + 1);
            }
            loop {
                if object {
                    i = string(b, i)?;
                    (b.get(i) == Some(&b':')).then_some(())?;
                    i += 1;
                }
                i = value(b, i)?;
                match *b.get(i)? {
                    b',' => i += 1,
                    c if c == close => return Some(i + 1),
                    _ => return None,
                }
            }
        }
        b'"' => string(b, i),
        b't' => b[i..].starts_with(b"true").then_some(i + 4),
        b'f' => b[i..].starts_with(b"false").then_some(i + 5),
        b'n' => b[i..].starts_with(b"null").then_some(i + 4),
        _ => number(b, i),
    }
}

fn string(b: &[u8], mut i: usize) -> Option<usize> {
    (b.get(i) == Some(&b'"')).then_some(())?;
    i += 1;
    loop {
        match *b.get(i)? {
            b'"' => return Some(i + 1),
            b'\\' => match *b.get(i + 1)? {
                b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => i += 2,
                b'u' => {
                    b.get(i + 2..i + 6)?.iter().all(u8::is_ascii_hexdigit).then_some(())?;
                    i += 6;
                }
                _ => return None,
            },
            c if c < 0x20 => return None,
            _ => i += 1,
        }
    }
}

fn number(b: &[u8], mut i: usize) -> Option<usize> {
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    match *b.get(i)? {
        b'0' => i += 1,
        b'1'..=b'9' => {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return None,
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        let d = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        (i > d).then_some(())?;
    }
    Some(i)
}

fuzz_target!(|data: &[u8]| {
    let Some((&ci, rest)) = data.split_first() else { return };
    for ci in [ci, 0x78, 0x72, 0x7A] {
        if let Ok(app) = Application::parse(ci, rest) {
            let text = Json::new(&app).rssi(Some(-1)).to_string();
            let b = text.as_bytes();
            assert_eq!(value(b, 0), Some(b.len()), "invalid JSON: {text}");
        }
    }
});
