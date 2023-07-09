# API Client for brouter

This rust crate contains a simple client for the API of
[brouter](https://brouter.de/brouter/), a routing engine based on openstreetmap
data.

## Usage

```rust

use brouter_client::{Brouter, Point};

let router = Brouter::default();

let gpx = router.broute(&[Point::new(52.3676, 4.9041), Point::new(52.0907, 5.1214)], &[], "trekking", None, None);
```
