```rust
use mbus_kit::wired::{Frame, req_ud2};
use mbus_kit::Application;

let n = req_ud2(5, false).encode(&mut request)?;
// … write request[..n] to the bus, read the reply …
if let Some((Frame::Long { ci, data, .. }, _)) = Frame::parse(&reply)? {
    for r in Application::parse(ci, data)?.records().into_iter().flatten() {
        // "volume flow · maximum · storage 5 — 0.113 m³/h"
        println!("{}", r?);
    }
}
```
