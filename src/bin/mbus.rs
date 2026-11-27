//! `mbus`: decode, read, search and listen to M-Bus and wireless M-Bus meters.
//!
//! ```text
//! cargo install mbus-kit --features cli
//! mbus decode captures.hex --key 0102…0F11 --json
//! mbus read /dev/ttyUSB0 5
//! mbus search /dev/ttyUSB0
//! mbus listen /dev/ttyUSB0 --stick im871a --mode t1 --key 0102…0F11
//! ```

use std::io::{self, BufRead, Read, Write};
use std::net::TcpStream;
use std::process::ExitCode;
use std::time::Duration;

use mbus_kit::app::Authenticity;
use mbus_kit::compact::{Format as Layout, FormatCache};
use mbus_kit::fragment::Reassembly;
use mbus_kit::json::{Json, JsonAddress, JsonString};
use mbus_kit::master::{Baud, SecondaryAddress, Target};
use mbus_kit::oms::Key;
use mbus_kit::serial::Bus;
use mbus_kit::stick::{LinkMode, Message, Receiver, Stick};
use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body, Header};

const USAGE: &str = "\
mbus — M-Bus and wireless M-Bus meters

usage:
  mbus decode [FILE] [--key HEX]... [--format a|b|stripped] [--json]
      hex telegrams, one per line (stdin without FILE); wired or wireless, detected;
      rtl-wmbus lines; `key …` and `format …` lines; compact frames and fragments joined
  mbus read DEVICE ADDRESS [--baud 300|2400|9600] [--key HEX]... [--json] [--no-stty]
      read a wired meter: ADDRESS is a primary address (0–250) or an 8-digit ID;
      DEVICE is a serial device or tcp:HOST:PORT
  mbus search DEVICE [--baud …] [--json] [--no-stty]
      find every meter on the bus by secondary address
  mbus listen DEVICE --stick im871a|iu891a|amb8465 [--mode t1|c1|c1t1|s1]
              [--key HEX]... [--json] [--setup] [--no-stty]
      receive radio telegrams through a stick (DEVICE `-`: a recorded stream on stdin);
      --setup prepares an Amber stick once (writes its flash)

The serial device is configured with stty (8E1 for M-Bus, 8N1 for sticks) unless
--no-stty is given.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(msg)) => {
            eprintln!("mbus: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Run(msg)) => {
            eprintln!("mbus: {msg}");
            ExitCode::FAILURE
        }
    }
}

enum Failure {
    Usage(String),
    Run(String),
}

impl<E: std::fmt::Display> From<E> for Failure {
    fn from(e: E) -> Self {
        Self::Run(e.to_string())
    }
}

/// Command-line options shared by every command.
#[derive(Default)]
struct Options {
    positional: Vec<String>,
    keys: Vec<Key>,
    json: bool,
    stty: bool,
    setup: bool,
    baud: Option<Baud>,
    format: Option<Format>,
    stick: Option<Stick>,
    mode: Option<LinkMode>,
}

fn options(args: &[String]) -> Result<Options, Failure> {
    let usage = |m: String| Failure::Usage(m);
    let mut o = Options { stty: true, ..Options::default() };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or_else(|| usage(format!("{name} needs a value")));
        match a.as_str() {
            "--json" => o.json = true,
            "--no-stty" => o.stty = false,
            "--setup" => o.setup = true,
            "--key" => {
                let v = value("--key")?;
                o.keys.push(Key::from_hex(&v).ok_or_else(|| usage("a key is 32 hex digits".into()))?);
            }
            "--baud" => {
                o.baud = Some(match value("--baud")?.as_str() {
                    "300" => Baud::B300,
                    "600" => Baud::B600,
                    "1200" => Baud::B1200,
                    "2400" => Baud::B2400,
                    "4800" => Baud::B4800,
                    "9600" => Baud::B9600,
                    "19200" => Baud::B19200,
                    "38400" => Baud::B38400,
                    other => return Err(usage(format!("unsupported baud rate {other}"))),
                });
            }
            "--format" => {
                o.format = Some(
                    parse_format(&value("--format")?).ok_or_else(|| usage("--format a|b|stripped".into()))?,
                )
            }
            "--stick" => {
                o.stick = Some(match value("--stick")?.as_str() {
                    "im871a" | "im170a" => Stick::Im871a,
                    "iu891a" | "im891a" | "iu893a" => Stick::Iu891a,
                    "amb8465" | "amb8665" | "amber" => Stick::Amb8465,
                    other => return Err(usage(format!("unknown stick {other}"))),
                });
            }
            "--mode" => {
                o.mode = Some(match value("--mode")?.to_ascii_lowercase().as_str() {
                    "s1" => LinkMode::S1,
                    "t1" => LinkMode::T1,
                    "c1" => LinkMode::C1,
                    "c1t1" | "ct" | "c1+t1" => LinkMode::C1T1,
                    other => return Err(usage(format!("unknown mode {other}"))),
                });
            }
            flag if flag.starts_with("--") => return Err(usage(format!("unknown option {flag}"))),
            other => o.positional.push(other.to_owned()),
        }
    }
    Ok(o)
}

fn parse_format(f: &str) -> Option<Format> {
    match f {
        "a" | "A" => Some(Format::A),
        "b" | "B" => Some(Format::B),
        "stripped" => Some(Format::Stripped),
        _ => None,
    }
}

fn run(args: &[String]) -> Result<(), Failure> {
    let (cmd, rest) = args.split_first().ok_or_else(|| Failure::Usage("no command".into()))?;
    let o = options(rest)?;
    match cmd.as_str() {
        "decode" => decode(&o),
        "read" => read(&o),
        "search" => search(&o),
        "listen" => listen(&o),
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        "--version" | "version" => {
            println!("mbus {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(Failure::Usage(format!("unknown command {other}"))),
    }
}

// ---- output

/// What a telegram passes through: keys, learnt layouts, the output style.
struct Ctx {
    keys: Vec<Key>,
    formats: FormatCache<32, 255>,
    json: bool,
    rssi: Option<i16>,
}

impl Ctx {
    fn new(o: &Options) -> Self {
        Self { keys: o.keys.clone(), formats: FormatCache::new(), json: o.json, rssi: None }
    }
}

/// Decrypt, join, expand and print one application.
fn handle(app: &Application<'_>, ctx: &mut Ctx, fragments: Option<&mut Reassembly<'_>>) {
    match &app.body {
        Body::Encrypted(e) => {
            for key in &ctx.keys.clone() {
                let mut plain = [0u8; 600];
                if let Ok(inner) = e.decrypt(key, &mut plain) {
                    return handle(&inner, ctx, fragments);
                }
            }
            if !ctx.keys.is_empty() && !ctx.json {
                println!("  no key opens it ({:?})", e.security);
            }
            show(app, ctx);
        }
        Body::Fragment(_) => match fragments {
            Some(f) => match f.push(app) {
                Ok(Some(message)) => handle(&message, ctx, None),
                Ok(None) => {
                    if !ctx.json {
                        println!("  fragment {} (waiting for more)", app.afl.map_or(0, |a| a.fragment_id()));
                    }
                }
                Err(e) => eprintln!("mbus: fragment: {e}"),
            },
            None => show(app, ctx),
        },
        Body::Compact(_) => {
            let mut out = [0u8; 600];
            match app.expand(&ctx.formats, &mut out) {
                Ok(full) => show(&full, ctx),
                Err(e) => {
                    if !ctx.json {
                        println!("  compact frame: {e} (a full frame of this meter teaches the layout)");
                    }
                    show(app, ctx);
                }
            }
        }
        Body::Format(layout) => {
            ctx.formats.insert(layout);
            show(app, ctx);
        }
        Body::Records(records) => {
            let mut layout = [0u8; 255];
            if let Some(l) = Layout::of(records, &mut layout) {
                ctx.formats.insert(&l);
            }
            show(app, ctx);
        }
        _ => show(app, ctx),
    }
}

fn show(app: &Application<'_>, ctx: &Ctx) {
    if ctx.json {
        println!("{}", Json::new(app).rssi(ctx.rssi));
        return;
    }
    let mut line = String::new();
    if let Some(m) = app.meter() {
        line.push_str(&m.to_string());
    }
    if let Some(dbm) = ctx.rssi {
        line.push_str(&format!(" · {dbm} dBm"));
    }
    match app.header {
        Header::Long(h) => line.push_str(&format!(" · access {}", h.short.access_number)),
        Header::Short(s) => line.push_str(&format!(" · access {}", s.access_number)),
        Header::None => {}
    }
    line.push_str(match app.authenticity {
        Authenticity::None => " · not authenticated",
        Authenticity::Encrypted => " · decrypted, not authenticated",
        Authenticity::Authenticated { .. } => " · authenticated",
    });
    if let Authenticity::Authenticated { counter } = app.authenticity {
        line.push_str(&format!(" (counter {counter})"));
    }
    println!("{}", line.trim_start_matches(" · "));
    match &app.body {
        Body::Records(records) => {
            let mut records = records.clone();
            for r in records.by_ref() {
                match r {
                    Ok(r) if app.authenticity != Authenticity::None && !r.from_ciphertext => {
                        println!("  {r}  (sent in the clear)");
                    }
                    Ok(r) => println!("  {r}"),
                    Err(e) => println!("  record error: {e}"),
                }
            }
            if let Some(m) = records.manufacturer_data().filter(|m| !m.is_empty()) {
                println!("  manufacturer data: {m:02X?}");
            }
        }
        Body::Encrypted(e) => {
            println!("  encrypted · {:?} · {} bytes (pass --key)", e.security, e.data.len())
        }
        Body::Compact(c) => println!("  compact frame · signature {:04X}", c.signature),
        Body::Format(f) => println!("  format frame · signature {:04X} (remembered)", f.signature),
        Body::Fragment(d) => println!("  fragment · {} bytes", d.len()),
        Body::ApplicationError(b) => println!("  application error {b:02X?}"),
        Body::Alarm(b) => println!("  alarm {b:02X?}"),
        Body::Empty => println!("  no application data"),
    }
}

// ---- decode

fn decode(o: &Options) -> Result<(), Failure> {
    let input: Box<dyn BufRead> = match o.positional.first() {
        Some(path) if path != "-" => Box::new(io::BufReader::new(std::fs::File::open(path)?)),
        _ => Box::new(io::BufReader::new(io::stdin())),
    };
    let mut ctx = Ctx::new(o);
    let mut format = o.format;
    let mut buf = vec![0u8; 16_384];
    let mut fragments = Reassembly::new(&mut buf);
    for line in input.lines() {
        let line = line?;
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with(['=', '!']) {
            continue;
        }
        if let Some(f) = line.strip_prefix("format ") {
            format = parse_format(f.trim());
            continue;
        }
        if let Some(k) = line.strip_prefix("key ") {
            ctx.keys.push(Key::from_hex(k.trim()).ok_or("a key line needs 32 hex digits")?);
            continue;
        }
        // rtl-wmbus: MODE;CRC_OK;3OUTOF6_OK;TIME;RSSI;RSSI;ID;0x<frame without CRCs>
        let (hex, this_format) = match line.rsplit_once(';') {
            Some((meta, frame)) if meta.split(';').nth(1) == Some("1") => {
                (frame.trim().trim_start_matches("0x").to_owned(), Some(Format::Stripped))
            }
            Some(_) => continue,
            None => (line.to_owned(), format),
        };
        let digits: String = hex.chars().filter(char::is_ascii_hexdigit).collect();
        if digits.len() % 2 != 0 {
            eprintln!("mbus: odd number of hex digits: {line}");
            continue;
        }
        let bytes: Vec<u8> = (0..digits.len() / 2)
            .filter_map(|i| u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).ok())
            .collect();
        if let Err(e) = decode_one(&bytes, this_format, &mut ctx, &mut fragments) {
            if ctx.json {
                println!("{{\"error\":{}}}", JsonString(&e.to_string()));
            } else {
                println!("error: {e}");
            }
        }
        if !ctx.json {
            println!();
        }
    }
    Ok(())
}

fn decode_one(
    bytes: &[u8],
    format: Option<Format>,
    ctx: &mut Ctx,
    fragments: &mut Reassembly<'_>,
) -> mbus_kit::Result<()> {
    let wired = matches!(bytes, [0x68, _, _, 0x68, ..] | [0x10, ..] | [0xE5]);
    if wired && format.is_none() {
        let Some((frame, _)) = Frame::parse(bytes)? else { return Err(mbus_kit::Error::UnexpectedEnd) };
        let Frame::Long { ci, data, .. } = frame else {
            if !ctx.json {
                println!("{frame:?}");
            }
            return Ok(());
        };
        handle(&Application::parse(ci, data)?, ctx, Some(fragments));
        return Ok(());
    }
    let formats: &[Format] = match &format {
        Some(f) => std::slice::from_ref(f),
        None => &[Format::A, Format::Stripped, Format::B],
    };
    let mut last = mbus_kit::Error::UnexpectedEnd;
    for &f in formats {
        let mut buf = [0u8; 600];
        match wireless::decode(bytes, f, &mut buf) {
            Ok(Some((frame, _))) => {
                handle(&frame.application()?, ctx, Some(fragments));
                return Ok(());
            }
            Ok(None) => last = mbus_kit::Error::UnexpectedEnd,
            Err(e) => last = e,
        }
    }
    Err(last)
}

// ---- wired

/// A serial device or a TCP gateway.
enum Port {
    Device(std::fs::File),
    Tcp(TcpStream),
}

impl Read for Port {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Device(f) => f.read(buf),
            Self::Tcp(s) => s.read(buf),
        }
    }
}

impl Write for Port {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Device(f) => f.write(buf),
            Self::Tcp(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Device(f) => f.flush(),
            Self::Tcp(s) => s.flush(),
        }
    }
}

/// `stty` for 8E1 (M-Bus) or 8N1 (sticks), raw, with a 100 ms read timeout.
fn stty(device: &str, baud: u32, even_parity: bool) -> Result<(), Failure> {
    let flag = if cfg!(target_os = "macos") { "-f" } else { "-F" };
    let parity: &[&str] = if even_parity { &["parenb", "-parodd"] } else { &["-parenb"] };
    let status = std::process::Command::new("stty")
        .arg(flag)
        .arg(device)
        .arg(baud.to_string())
        .args(["cs8", "-cstopb"])
        .args(parity)
        .args(["raw", "-echo", "min", "0", "time", "1"])
        .status()
        .map_err(|e| format!("stty: {e} (configure the port yourself and pass --no-stty)"))?;
    if !status.success() {
        return Err(Failure::Run(format!("stty could not configure {device} (pass --no-stty to skip)")));
    }
    Ok(())
}

fn open_bus(o: &Options) -> Result<Bus<Port>, Failure> {
    let device = o.positional.first().ok_or_else(|| Failure::Usage("which device?".into()))?;
    let baud = o.baud.unwrap_or(Baud::B2400);
    let (port, slack) = if let Some(addr) = device.strip_prefix("tcp:") {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_millis(10)))?;
        (Port::Tcp(stream), 200)
    } else {
        if o.stty && cfg!(unix) {
            stty(device, baud.bits_per_second(), true)?;
        }
        (Port::Device(std::fs::OpenOptions::new().read(true).write(true).open(device)?), 50)
    };
    Ok(Bus::new(port, baud).slack(Duration::from_millis(slack)))
}

fn read(o: &Options) -> Result<(), Failure> {
    let what = o
        .positional
        .get(1)
        .ok_or_else(|| Failure::Usage("which address? (0–250 or an 8-digit ID)".into()))?;
    let target = if what.len() == 8 {
        let id: u32 = what.parse().map_err(|_| Failure::Usage(format!("{what} is not an 8-digit ID")))?;
        Target::Secondary(SecondaryAddress::id(id).ok_or("not an 8-digit ID")?)
    } else {
        let a: u8 = what.parse().map_err(|_| Failure::Usage(format!("{what} is not a primary address")))?;
        Target::Primary(a)
    };
    let mut bus = open_bus(o)?;
    let mut ctx = Ctx::new(o);
    let mut buf = vec![0u8; 16_384];
    let mut fragments = Reassembly::new(&mut buf);
    for telegram in bus.readout(target)? {
        handle(&telegram.application()?, &mut ctx, Some(&mut fragments));
    }
    Ok(())
}

fn search(o: &Options) -> Result<(), Failure> {
    let mut bus = open_bus(o)?;
    if !o.json {
        eprintln!("searching (every empty probe waits for the response timeout) …");
    }
    let found = bus.search()?;
    for meter in &found.meters {
        if o.json {
            println!("{{\"meter\":{}}}", JsonAddress(meter));
        } else {
            println!("{meter}");
        }
    }
    for id in &found.ambiguous {
        if o.json {
            println!("{{\"ambiguous\":{}}}", JsonString(&id.to_string()));
        } else {
            println!("{id}: several meters share this ID (use their fabrication numbers)");
        }
    }
    if !found.complete {
        eprintln!("mbus: the search gave up at its selection limit: is the line stuck?");
    }
    Ok(())
}

// ---- radio

fn listen(o: &Options) -> Result<(), Failure> {
    let stick = o.stick.ok_or_else(|| Failure::Usage("which stick? --stick im871a|iu891a|amb8465".into()))?;
    let mode = o.mode.unwrap_or(LinkMode::T1);
    let device = o.positional.first().map_or("-", String::as_str);
    let mut port: Box<dyn Read> = if device == "-" {
        Box::new(io::stdin())
    } else {
        if o.stty && cfg!(unix) {
            stty(device, stick.baud(), false)?;
        }
        let mut file = std::fs::OpenOptions::new().read(true).write(true).open(device)?;
        if o.setup {
            for command in stick.setup() {
                file.write_all(command.bytes())?;
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        let listen =
            stick.listen(mode).ok_or_else(|| Failure::Run(format!("{stick:?} cannot receive {mode}")))?;
        file.write_all(listen.bytes())?;
        eprintln!("listening for {mode} on {device} ({stick:?}, {} Bd) …", stick.baud());
        Box::new(file)
    };
    let mut ctx = Ctx::new(o);
    let mut rx = Receiver::new(stick);
    let mut buf = vec![0u8; 16_384];
    let mut fragments = Reassembly::new(&mut buf);
    let mut chunk = [0u8; 512];
    loop {
        let n = match port.read(&mut chunk) {
            Ok(0) if device == "-" => return Ok(()),
            Ok(n) => n,
            Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => 0,
            Err(e) => return Err(e.into()),
        };
        let mut rest = &chunk[..n];
        loop {
            let taken = rx.extend(rest);
            rest = &rest[taken..];
            while let Some(message) = rx.next_message() {
                match message {
                    Ok(Message::Telegram(t)) => {
                        let mut frame = [0u8; 600];
                        match wireless::decode(t.frame, Format::Stripped, &mut frame) {
                            Ok(Some((f, _))) => match f.application() {
                                Ok(app) => {
                                    ctx.rssi = t.rssi_dbm;
                                    handle(&app, &mut ctx, Some(&mut fragments));
                                }
                                Err(e) => eprintln!("mbus: {}: {e}", f.address),
                            },
                            Ok(None) => eprintln!("mbus: truncated telegram"),
                            Err(e) => eprintln!("mbus: telegram: {e}"),
                        }
                    }
                    Ok(Message::Response { group, id, payload }) => {
                        if !o.json {
                            eprintln!("stick: {group:02X}/{id:02X} {payload:02X?}");
                        }
                    }
                    Err(e) => eprintln!("stick: skipped ({e})"),
                }
            }
            if rest.is_empty() {
                break;
            }
        }
    }
}
