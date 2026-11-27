//! Decode a capture: hex on stdin, records out.
//!
//! ```text
//! echo 68 1F 1F 68 08 02 72 … 18 16 | cargo run --example decode
//! cargo run --example decode -- --wireless < frame.hex     # format A, from the L field
//! cargo run --example decode -- --wireless-b < frame.hex   # format B
//! ```
//!
//! Whitespace in the hex is ignored. Encrypted wireless telegrams (OMS mode 5/7) are
//! reported, not decrypted: that is a later milestone.

use std::io::Read;

use mbus_kit::wired::Frame;
use mbus_kit::wireless::{self, Format};
use mbus_kit::{Application, Body, Header};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let format = if args.iter().any(|a| a == "--wireless") {
        Some(Format::A)
    } else if args.iter().any(|a| a == "--wireless-b") {
        Some(Format::B)
    } else {
        None
    };

    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text)?;
    let hex: String = text.chars().filter(char::is_ascii_hexdigit).collect();
    let bytes = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()?;

    let mut out = [0u8; 512];
    let (ci, data) = match format {
        Some(f) => {
            let (frame, _) = wireless::decode(&bytes, f, &mut out)?.ok_or("incomplete wireless frame")?;
            let a = frame.address;
            println!(
                "wireless C=0x{:02x} {} {:08} {} v{}",
                frame.c,
                a.manufacturer,
                a.id_number().unwrap_or(0),
                a.medium,
                a.version
            );
            (frame.ci, frame.payload)
        }
        None => match Frame::parse(&bytes)?.ok_or("incomplete frame")?.0 {
            Frame::Long { c, a, ci, data } => {
                println!("wired C=0x{c:02x} A={a}");
                (ci, data)
            }
            other => {
                println!("{other:?}");
                return Ok(());
            }
        },
    };

    let app = Application::parse(ci, data)?;
    match app.header {
        Header::Long(h) => println!(
            "long header: {} {:08} {} v{}, access {}",
            h.manufacturer,
            h.id_number().unwrap_or(0),
            h.medium,
            h.version,
            h.short.access_number
        ),
        Header::Short(s) => println!("short header: access {}, status 0x{:02x}", s.access_number, s.status),
        Header::None => {}
    }
    match app.body {
        Body::Encrypted { security, data } => {
            println!(
                "encrypted: mode {}, {} blocks, {} bytes",
                security.mode,
                security.encrypted_blocks,
                data.len()
            );
        }
        Body::Records(mut records) => {
            for record in records.by_ref() {
                match record {
                    Ok(r) => {
                        let value = r.quantity().map_or_else(|| format!("{:?}", r.value), |q| q.to_string());
                        println!(
                            "  {:<22} {value}  (storage {}, tariff {})",
                            r.name(),
                            r.dib.storage,
                            r.dib.tariff
                        );
                    }
                    Err(e) => eprintln!("  record error: {e}"),
                }
            }
            if let Some(m) = records.manufacturer_data() {
                println!("  manufacturer data: {m:02x?}");
            }
        }
    }
    Ok(())
}
