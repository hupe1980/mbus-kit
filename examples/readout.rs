//! The water meter telegram from the M-Bus documentation, decoded.
//!
//! `cargo run --example readout`

use mbus_kit::wired::Frame;
use mbus_kit::{Application, Body, Header};

const TELEGRAM: [u8; 37] = [
    0x68, 0x1F, 0x1F, 0x68, 0x08, 0x02, 0x72, 0x78, 0x56, 0x34, 0x12, 0x24, 0x40, 0x01, 0x07, 0x55, 0x00,
    0x00, 0x00, 0x03, 0x13, 0x15, 0x31, 0x00, 0xDA, 0x02, 0x3B, 0x13, 0x01, 0x8B, 0x60, 0x04, 0x37, 0x18,
    0x02, 0x18, 0x16,
];

fn main() -> Result<(), mbus_kit::Error> {
    let Some((Frame::Long { ci, data, .. }, _)) = Frame::parse(&TELEGRAM)? else {
        return Err(mbus_kit::Error::UnexpectedEnd);
    };
    let app = Application::parse(ci, data)?;
    if let Header::Long(h) = app.header {
        println!(
            "{} meter {:08} by {}, version {}",
            h.medium,
            h.id_number().unwrap_or(0),
            h.manufacturer,
            h.version
        );
    }
    if let Body::Records(records) = app.body {
        for record in records {
            let r = record?;
            let value = r.quantity().map_or_else(|| format!("{:?}", r.value), |q| q.to_string());
            println!(
                "  {:<22} {:<16} {:?}, storage {}, tariff {}, subunit {}",
                r.name(),
                value,
                r.dib.function,
                r.dib.storage,
                r.dib.tariff,
                r.dib.subunit
            );
        }
    }
    Ok(())
}
