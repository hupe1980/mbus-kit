```rust
use mbus_kit::wireless::{Format, decode};
use mbus_kit::{Application, Body};

let mut buf = [0u8; 256];
if let Some((frame, _)) = decode(&raw, Format::A, &mut buf)? {
    println!("{} {:?}", frame.address.manufacturer,
             frame.address.id_number());
    match Application::parse(frame.ci, frame.payload)?.body {
        Body::Records(r) => { /* … */ }
        Body::Encrypted { security, .. } => { /* OMS mode 5/7 */ }
    }
}
```
