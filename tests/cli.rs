//! The `mbus` tool (spec 006 US2), run as a process: decode with keys and fragments, JSON
//! lines, a stick stream on stdin, a wired readout through a simulated TCP gateway, and
//! usage errors.
#![cfg(feature = "cli")]

mod sim;

use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};

fn mbus(args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mbus"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

const CORPUS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/corpus/wireless/");

#[test]
fn decode_a_fixture_with_its_key_as_text_and_json() {
    let o = mbus(&["decode", &format!("{CORPUS}oms-n2.3-mode7-afl.hex")], b"");
    assert!(o.status.success());
    let text = stdout(&o);
    assert!(text.contains("ELS 12345678 gas v51 · access 117 · authenticated (counter 2739)"), "{text}");
    assert!(text.contains("  volume — 28504.27 m³"), "{text}");
    let o = mbus(&["decode", &format!("{CORPUS}oms-n2.3-mode7-afl.hex"), "--json"], b"");
    let json = stdout(&o);
    assert_eq!(json.lines().count(), 1, "{json}");
    assert!(json.starts_with(r#"{"meter":{"manufacturer":"ELS","id":"12345678""#), "{json}");
    assert!(json.contains(r#""value":28504.27,"unit":"m3""#), "{json}");
    // 006-json-and-cli/SR-002: what was verified, on the object and per record.
    assert!(json.contains(r#""authenticity":"authenticated","counter":2739"#), "{json}");
    assert!(json.contains(r#""encrypted":true"#), "{json}");
}

#[test]
fn fragments_on_consecutive_lines_become_one_telegram() {
    // OMS Annex N N.3, three fragments; the key on the command line.
    let n3 = include_str!("fragment.rs");
    let frames: Vec<&str> = n3
        .lines()
        .skip_while(|l| !l.starts_with("const N3"))
        .skip(1)
        .take(3)
        .map(|l| l.trim().trim_matches(|c| c == '"' || c == ','))
        .collect();
    let input = frames.join("\n");
    let o = mbus(&["decode", "--key", "000102030405060708090A0B0C0D0E0F", "--json"], input.as_bytes());
    let json = stdout(&o);
    assert_eq!(json.lines().count(), 1, "{json}");
    assert!(json.contains(r#""authenticity":"authenticated","counter":2739"#), "{json}");
}

#[test]
fn a_stick_stream_on_stdin() {
    let bytes = [
        0xA5u8, 0xC2, 0x03, 0x19, 0x44, 0x24, 0x23, 0x85, 0x07, 0x60, 0x14, 0x40, 0x18, 0xA1, 0x51, 0x49,
        0x00, 0x13, 0x53, 0x55, 0xB8, 0x53, 0x18, 0x92, 0xDF, 0x2B, 0x27, 0x37, 0x34, 0x56, 0x60, 0xEB,
    ];
    let o = mbus(&["listen", "-", "--stick", "im871a"], &bytes);
    assert!(o.status.success(), "{o:?}");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("HYD 14600785 pressure v64"), "{err}");
}

/// A simulated TCP gateway in front of meter 12345678 at primary 1, sending `telegrams`.
fn gateway(telegrams: u8) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bus = sim::Bus::new(&[12_345_678]);
        bus.meters[0].telegrams = telegrams;
        let mut buf = [0u8; 64];
        let mut pending = Vec::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            if let Ok(Some((_, len))) = mbus_kit::wired::Frame::parse(&pending) {
                let reply = bus.send(&pending[..len]);
                pending.drain(..len);
                stream.write_all(&reply).unwrap();
            }
        }
    });
    (addr, handle)
}

#[test]
fn read_a_meter_through_a_tcp_gateway() {
    // SC-003 (006): one command from address to records, here against a simulated gateway.
    let (addr, gateway) = gateway(1);
    let o = mbus(&["read", &format!("tcp:{addr}"), "1", "--json"], b"");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let json = stdout(&o);
    assert!(json.contains(r#""meter":{"manufacturer":"KAM","id":"12345678""#), "{json}");
    gateway.join().unwrap();
}

// 007-readout-integrity/SR-002: an incomplete readout is printed, but never a success.
#[test]
fn an_incomplete_readout_fails_after_printing_what_was_read() {
    // 20 telegrams, the default limit is 16.
    let (addr, gateway) = gateway(20);
    let o = mbus(&["read", &format!("tcp:{addr}"), "1", "--json"], b"");
    assert_eq!(o.status.code(), Some(1));
    let json = stdout(&o);
    assert_eq!(json.lines().filter(|l| l.starts_with(r#"{"meter""#)).count(), 16, "{json}");
    let last = json.lines().last().unwrap();
    assert!(
        last.starts_with(r#"{"error":"incomplete readout after 16 telegram(s): telegram limit reached"#),
        "{last}"
    );
    assert!(String::from_utf8_lossy(&o.stderr).contains("incomplete readout"));
    gateway.join().unwrap();
    let o = mbus(&["read", "tcp:127.0.0.1:1", "251"], b"");
    assert_eq!(o.status.code(), Some(2), "251 is refused before connecting");
    assert!(String::from_utf8_lossy(&o.stderr).contains("not a primary address (0–250)"));
}

#[test]
fn usage_errors_say_what_is_wrong() {
    let o = mbus(&["listen", "-"], b"");
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("which stick?"));
    // 006-json-and-cli/SR-003: a rejected key is not echoed back.
    let o = mbus(&["decode", "--key", "0102030405060708090A0B0C0D0E0F1"], b"");
    assert_eq!(o.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&o.stderr).contains("0102030405"));
    let o = mbus(&["--version"], b"");
    assert!(stdout(&o).starts_with("mbus "));
    let o = mbus(&["decode", "--json"], b"0102\n");
    assert!(stdout(&o).starts_with(r#"{"error":"#), "{}", stdout(&o));
}

#[test]
fn decode_streams_line_by_line() {
    // 006-json-and-cli/SR-004: each telegram comes out while stdin is still open, so the
    // tool never holds the whole input.
    use std::io::BufRead;
    let mut child = Command::new(env!("CARGO_BIN_EXE_mbus"))
        .args(["decode", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
    for _ in 0..3 {
        stdin.write_all(b"68 03 03 68 08 01 78 81 16\n").unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        assert!(line.starts_with(r#"{"meter":null,"ci":"78""#), "{line}");
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
}

#[test]
fn a_reader_that_stops_early_is_no_error() {
    // `mbus decode … | head -1`: the pipe closes, the tool ends quietly.
    let mut child = Command::new(env!("CARGO_BIN_EXE_mbus"))
        .arg("decode")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut stdin = child.stdin.take().unwrap();
    for _ in 0..1000 {
        if stdin.write_all(b"68 03 03 68 08 01 78 81 16\n").is_err() {
            break;
        }
    }
    drop(stdin);
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(!String::from_utf8_lossy(&o.stderr).contains("panicked"));
}
