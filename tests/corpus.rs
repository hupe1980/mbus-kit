//! The real-meter corpus (spec 001 US4, spec 002 US6): every frame in `tests/corpus/`
//! decodes, and every record reads as the line in its fixture.
//!
//! A fixture is the frame in hex plus:
//!
//! ```text
//! # comment: source, licence, the independent expected values
//! format a|b|stripped        wireless frame format (absent: a wired frame)
//! key 0102…                  the meter key (feature `oms`)
//! ! name: description        a deviation from the standard, named
//! = line                     an expected output line, in order
//! ```
//!
//! The `=` lines were produced by this crate and then checked by hand, record by record,
//! against the values the source documents (libmbus `.norm.xml`, wmbusmeters JSON, OMS
//! Annex N, jMBus asserts); differences are deviations (`!`) or were fixed. Regenerate
//! after a deliberate change with `MBUS_BLESS=1 cargo test --all-features --test corpus`
//! and review the diff.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use mbus_kit::app::Authenticity;
use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body};

struct Fixture {
    path: PathBuf,
    text: String,
    frame: Vec<u8>,
    format: Option<Format>,
    key: Option<String>,
    expected: Vec<String>,
}

fn load(path: &Path) -> Fixture {
    let text = fs::read_to_string(path).unwrap();
    let mut f = Fixture {
        path: path.into(),
        text: text.clone(),
        frame: Vec::new(),
        format: None,
        key: None,
        expected: Vec::new(),
    };
    for line in text.lines() {
        if let Some(e) = line.strip_prefix("= ") {
            f.expected.push(e.to_owned());
        } else if let Some(fmt) = line.strip_prefix("format ") {
            f.format = Some(match fmt.trim() {
                "a" => Format::A,
                "b" => Format::B,
                "stripped" => Format::Stripped,
                other => panic!("{}: format {other}", path.display()),
            });
        } else if let Some(k) = line.strip_prefix("key ") {
            f.key = Some(k.trim().to_owned());
        } else if !line.starts_with('#') && !line.starts_with('!') && !line.trim().is_empty() {
            let hex: String = line.chars().filter(char::is_ascii_hexdigit).collect();
            f.frame =
                (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
        }
    }
    f
}

/// Render what the crate makes of the fixture, one line per fact.
fn render(f: &Fixture) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    let app = match f.format {
        None => {
            let (frame, used) = Frame::parse(&f.frame).unwrap().expect("a complete wired frame");
            assert_eq!(used, f.frame.len(), "{}: trailing bytes", f.path.display());
            let Frame::Long { c, a, ci, data } = frame else {
                panic!("{}: not a long frame", f.path.display())
            };
            out.push(format!("wired C {c:02X} address {a}"));
            Application::parse(ci, data)
        }
        Some(format) => {
            let (frame, _) =
                wireless::decode(&f.frame, format, &mut buf).unwrap().expect("a complete wireless frame");
            out.push(format!("wireless C {:02X} {}", frame.c, frame.address));
            frame.application()
        }
    };
    let app = match app {
        Ok(app) => app,
        Err(e) => {
            out.push(format!("error: {e}"));
            return out;
        }
    };
    let mut plain = [0u8; 512];
    let app = open(f, app, &mut plain, &mut out);
    if let Some(meter) = app.meter().filter(|m| out.first().is_none_or(|l| !l.ends_with(&m.to_string()))) {
        out.push(format!("meter {meter}"));
    }
    out.push(
        match app.authenticity {
            Authenticity::None => "not authenticated",
            Authenticity::Encrypted => "decrypted, not authenticated",
            Authenticity::Authenticated { .. } => "authenticated",
        }
        .to_owned(),
    );
    match app.body {
        Body::Records(mut records) => {
            for r in records.by_ref() {
                out.push(match r {
                    Ok(r) => r.to_string(),
                    Err(e) => format!("error: {e}"),
                });
            }
            if let Some(m) = records.manufacturer_data().filter(|m| !m.is_empty()) {
                let mut line = String::from("manufacturer data");
                for b in m {
                    let _ = write!(line, " {b:02X}");
                }
                if records.more_follows() {
                    line.push_str(" (more follows)");
                }
                out.push(line);
            }
        }
        Body::Encrypted(e) => out.push(format!("encrypted {:?}", e.security)),
        Body::ApplicationError(code) => out.push(format!("application error {code:02X?}")),
        Body::Alarm(status) => out.push(format!("alarm {status:02X?}")),
        Body::Empty => out.push("no application data".to_owned()),
        Body::Compact(c) => {
            out.push(format!("compact frame, signature {:04X}, {} data bytes", c.signature, c.data.len()))
        }
        Body::Format(f) => out.push(format!("format frame, signature {:04X}", f.signature)),
        Body::Fragment(d) => out.push(format!("fragment, {} bytes", d.len())),
    }
    out
}

#[cfg(feature = "oms")]
fn open<'a>(f: &Fixture, app: Application<'a>, plain: &'a mut [u8], _out: &mut [String]) -> Application<'a> {
    let (Some(key), Body::Encrypted(e)) = (&f.key, &app.body) else { return app };
    let key = mbus_kit::oms::Key::from_hex(key).expect("a 128-bit key");
    e.decrypt(&key, plain).unwrap_or_else(|err| panic!("{}: {err}", f.path.display()))
}

#[cfg(not(feature = "oms"))]
fn open<'a>(_: &Fixture, app: Application<'a>, _: &'a mut [u8], _: &mut [String]) -> Application<'a> {
    app
}

fn fixtures() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut all = Vec::new();
    for dir in ["wired", "wireless"] {
        for e in fs::read_dir(root.join(dir)).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "hex") {
                all.push(p);
            }
        }
    }
    all.sort();
    all
}

#[test]
fn every_corpus_frame_decodes_as_documented() {
    let bless = std::env::var_os("MBUS_BLESS").is_some();
    let mut failures = Vec::new();
    let mut manufacturers = std::collections::BTreeSet::new();
    for path in fixtures() {
        let f = load(&path);
        if f.key.is_some() && cfg!(not(feature = "oms")) {
            continue;
        }
        let got = render(&f);
        if let Some(first) = got.first() {
            // "wired …" lines carry no manufacturer; the meter line does.
            for line in &got {
                if let Some(m) = line
                    .strip_prefix("meter ")
                    .or_else(|| line.strip_prefix("wireless C ").and_then(|l| l.get(3..)))
                {
                    manufacturers.insert(m.chars().take(3).collect::<String>());
                }
            }
            let _ = first;
        }
        if bless {
            let mut text: String =
                f.text.lines().filter(|l| !l.starts_with("= ")).map(|l| format!("{l}\n")).collect();
            for line in &got {
                let _ = writeln!(text, "= {line}");
            }
            fs::write(&path, text).unwrap();
        } else if got != f.expected {
            failures.push(format!(
                "{}\n  expected:\n    {}\n  got:\n    {}",
                path.display(),
                f.expected.join("\n    "),
                got.join("\n    ")
            ));
        }
    }
    assert!(failures.is_empty(), "{} corpus frame(s) differ:\n{}", failures.len(), failures.join("\n"));
    // SC-003 (001): ≥ 5 manufacturers wired; SC-005 (002): ≥ 4 wireless. Together ≥ 9.
    assert!(manufacturers.len() >= 9, "manufacturers: {manufacturers:?}");
}

#[test]
fn every_wired_record_is_described() {
    // SC-002 (001): no record of the wired corpus decodes to an error, and none is
    // "reserved", except where a fixture names the deviation.
    for path in fixtures().into_iter().filter(|p| p.to_string_lossy().contains("/wired/")) {
        let f = load(&path);
        for line in render(&f) {
            let named = f.text.lines().any(|l| l.starts_with('!'));
            assert!(
                named || (!line.starts_with("error:") && !line.contains("reserved")),
                "{}: {line}",
                path.display()
            );
        }
    }
}
