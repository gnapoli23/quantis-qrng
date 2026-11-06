# quantis-qrng

Unofficial pure-Rust driver for ID Quantique **Quantis** quantum random number
generators (QRNG).

> [!WARNING]
> Early development. The Quantis USB commands work on real hardware, reading
> random data still has to be tested on a real device. No PCIe yet. Don't use
> it in production.

## Goals

- **Quantis USB** in pure Rust, without C toolchain, `libusb`, `cmake` or
  `bindgen`.
- **Quantis PCIe** with the ID Quantique kernel driver (`/dev/qrandomN`).
- A safe API: a read fills the whole buffer, or it fails.
- A `qrng` command-line tool.

## Installation

Library:

```sh
cargo add quantis-qrng@0.1.0-alpha.1
cargo add quantis-qrng@0.1.0-alpha.1 --features rand_core   # with `rand` support
```

`qrng` command-line tool:

```sh
cargo install quantis-qrng --version 0.1.0-alpha.1 --features cli
```

Only pre-releases for now, cargo needs the explicit version. From 0.1.0 it
won't be needed.

MSRV is Rust 1.85.

### Platforms

| OS      | Status |
|---------|--------|
| Linux   | tested |
| macOS   | should work (no driver needed), not tested yet |
| Windows | not tested yet, needs the WinUSB driver |

On Windows the device must use the **WinUSB** driver, not the ID Quantique
one. It can be replaced with [Zadig](https://zadig.akeo.ie), but then the
ID Quantique software doesn't work anymore. Use `qrng read -o FILE` instead of
`>`, the shell can change binary data (Windows PowerShell does).

### Device access on Linux

Quantis USB has no kernel driver, by default only root can open the device.
The udev rule in [`contrib/udev`](contrib/udev/70-quantis-qrng.rules) gives
access to the `plugdev` group and to the user logged in at the machine:

```sh
sudo install -m 0644 contrib/udev/70-quantis-qrng.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=0aba
sudo usermod -aG plugdev "$USER"   # then log out and log in again
```

If it still doesn't work, plug the device again. Without the rule
`qrng list` works, `qrng probe` fails with "permission denied".

Only one process at a time can use the device, `fuser -v /dev/bus/usb/BBB/DDD`
shows who is using it (bus and device from `lsusb -d 0aba:0102`).

## Usage

### Command line

```sh
qrng list                          # attached devices and serial numbers
qrng probe                         # info and registers of the device
qrng read -n 1M > random.bin       # 1 MiB of random bytes
qrng read -n 1M -o random.bin      # same, without the shell (better on Windows)
qrng read -n 64 --hex              # 64 bytes as hex
qrng read -n 10M | ent             # pipe to another program
qrng probe --serial 1234567890     # pick a device if more are attached
```

`-n` defaults to 1024, with suffixes `K`, `M` and `G` (powers of 1024), no
upper limit.

`qrng read` doesn't write binary data to a terminal, use `--hex`, `-o FILE` or
a redirect. If the read fails the output stops (never padded) and the exit
status is 1.

### Library

```rust,no_run
use quantis_qrng::usb::QuantisUsb;

fn main() -> Result<(), quantis_qrng::Error> {
    // only one device attached, otherwise use `QuantisUsb::open_serial`
    let mut quantis = QuantisUsb::open()?;
    println!("serial number: {:?}", quantis.serial_number());
    println!("data rate: {} bytes/s", quantis.modules_rate()?);

    let mut seed = [0u8; 32];
    quantis.fill_bytes(&mut seed)?;
    Ok(())
}
```

`fill_bytes` returns `Ok` only if the whole buffer has been filled, on error
the buffer is set to zero. Like the original library, it checks the module
status before every transfer.

`QuantisUsb` also implements `std::io::Read`, so it works with `io::copy`,
`BufReader` and any `impl Read`:

```rust,no_run
use std::{fs::File, io::{self, Read}};
use quantis_qrng::usb::QuantisUsb;

fn main() -> io::Result<()> {
    let quantis = QuantisUsb::open()?;

    // the device never ends, `take` is needed
    let mut file = File::create("random.bin")?;
    io::copy(&mut quantis.take(1024 * 1024), &mut file)?;
    Ok(())
}
```

`read_exact` works like `fill_bytes`. The errors become `io::Error` with the
closest `io::ErrorKind`, the original `Error` is inside.

### `rand`

With the `rand_core` feature `QuantisUsb` implements `TryRng`, every call reads
from the device (good for seeds). `QuantisRng` adds a 16 KiB buffer for many
small values, every byte is returned once and then cleared:

```rust,no_run
use quantis_qrng::usb::{QuantisRng, QuantisUsb};
use rand::{RngExt, SeedableRng, rand_core::UnwrapErr, rngs::StdRng};

fn main() -> Result<(), quantis_qrng::Error> {
    // seed
    let mut quantis = QuantisUsb::open()?;
    let std_rng = StdRng::try_from_rng(&mut quantis)?;

    // many small values, `UnwrapErr` panics on device errors
    let mut rng = QuantisRng::new(quantis);
    let dice = UnwrapErr(&mut rng).random_range(1..=6);
    Ok(())
}
```

No `TryCryptoRng`, the raw output can have a small bias: for keys, seed a
CSPRNG.

## Differences from the original library

Same behaviour of the ID Quantique C library (release 24.4.2), without its
bugs. There is no official repository, a copy of the sources is in
[sanddweller/QRNGPcieDriver](https://github.com/sanddweller/QRNGPcieDriver/tree/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src).

- After a short USB transfer
  [`QuantisUsbRead`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L564-L609)
  returns success with the buffer partially filled, and `EasyQuantis` pads the
  file with zeros. Here the buffer is always filled, or the read fails.
- The device is selected by serial number, not by index. The index check of
  [`QuantisUsbOpen`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L439)
  is off by one and reads uninitialized memory.
- `qrng list` doesn't open the devices (`EasyQuantis -l` does), so it doesn't
  get in the way of a running service.

## Wishlist

After 0.1, in no particular order.

- **Uniform numbers**: integers in a range and floats in `[0, 1)`, also in
  `qrng read` (`--integers --min --max`, `--floats`). The original library gets
  them wrong: scaled integers
  ([`QuantisReadScaledInt`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Quantis_C.c#L773-L806))
  are biased (negative samples are never discarded and `max - min + 1` can
  overflow) and doubles
  ([`ConvertToDouble_01`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Conversion.c#L61-L66))
  can be `1.0`.
- **Randomness extraction**, the matrix extractor of `QuantisExtensions`
  (removes the bias, so a proper `TryCryptoRng`).
- **Quantis PCIe** with the ID Quantique kernel driver (`/dev/qrandomN`). The
  registers need `ioctl`, and the crate forbids `unsafe`. The original read loop
  ([`QuantisPciRead`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisPci_Unix.c#L463-L501))
  never ends if `read()` returns 0, to avoid here.
- **Public transport layer**, only if someone needs it. Internal for now, so
  it can change without breaking anything.
- **Async API**, nusb is already async.
- **More `qrng` commands**: enable/disable the module, clear the AIS 31 flag.
- **macOS and Windows** tests.

## Contributing

Commits follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `refactor:`, `docs:`, ..., `!` for breaking changes).
[CHANGELOG.md](CHANGELOG.md) is generated with [git-cliff](https://git-cliff.org)
at every release: `git cliff --unreleased --tag vX.Y.Z --prepend CHANGELOG.md`.

## Disclaimer

This project is **not affiliated with or endorsed by ID Quantique SA**.
"Quantis" is a trademark of ID Quantique SA. This crate is a port of the
BSD-licensed Quantis C library, see [NOTICE](NOTICE).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
