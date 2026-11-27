```rust
use mbus_kit::master::{Baud, Readout, SecondaryAddress, Target};
use mbus_kit::serial::Bus;

let mut bus = Bus::new(port, Baud::B2400);              // a serial port or a TcpStream
let meter = SecondaryAddress::pattern("12345678").unwrap();
for telegram in bus.run_readout(Readout::new(Target::Primary(5)).expect_meter(meter))? {
    for r in telegram.application()?.records().into_iter().flatten() {
        println!("{}", r?);                               // "volume flow · maximum · storage 5 — 0.113 m³/h"
    }
}
```
