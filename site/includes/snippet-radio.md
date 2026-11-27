```rust
use mbus_kit::oms::Key;
use mbus_kit::wireless::{self, Format};

let key = Key::from_hex(KEY_SHEET).unwrap();
let mut buf = [0u8; 512];
if let Some((t, _)) = wireless::open(&raw, Format::A, &key, &mut buf)? {
    // ELS 12345678 gas v51 · Authenticated { counter: 2739 }
    println!("{} · {:?}", t.address, t.app.authenticity);
    for r in t.app.records().into_iter().flatten() {
        println!("{}", r?); // "volume — 28504.27 m³"
    }
}
```
