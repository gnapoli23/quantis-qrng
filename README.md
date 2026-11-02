# quantis-qrng

Unofficial pure-Rust driver for ID Quantique **Quantis** quantum random number
generators (QRNG).

> [!WARNING]
> Early development. Version 0.0.x only documents the device protocol; device
> access is not implemented yet. Do not use it in production.

## Goals

- **Quantis USB** support in pure Rust, with no C toolchain, `libusb`, `cmake` or
  `bindgen` needed to build it.
- **Quantis PCIe** support through the ID Quantique kernel driver (`/dev/qrandomN`).
- A safe API: a read either fills the whole buffer with device output or returns
  an error.
- A `qrng` command-line tool.

## Disclaimer

This project is **not affiliated with or endorsed by ID Quantique SA**.
"Quantis" is a trademark of ID Quantique SA. This crate is a port of the
BSD-licensed Quantis C library; see [NOTICE](NOTICE).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
