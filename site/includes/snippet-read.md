```rust
use mbus_kit::wired::{Frame, req_ud2};
use mbus_kit::{Application, Body};

let n = req_ud2(5, false).encode(&mut request)?;
// … write request[..n] to the bus, read the reply …
if let Some((Frame::Long { ci, data, .. }, _)) = Frame::parse(&reply)? {
    if let Body::Records(records) = Application::parse(ci, data)?.body {
        for r in records.flatten() {
            // "volume = 12.565 m³": exact
            println!("{} = {:?}", r.name(), r.quantity());
        }
    }
}
```
