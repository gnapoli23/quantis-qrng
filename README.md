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

MSRV is Rust 1.85.

### Platforms

| OS      | Status |
|---------|--------|
| Linux   | tested |
| macOS   | should work (no driver needed), not tested yet |
| Windows | not tested yet, needs the WinUSB driver |

On Windows the device must use the **WinUSB** driver, not the ID Quantique
one.

### Device access on Linux

Quantis USB has no kernel driver, by default only root can open the device.
The udev rule
[`contrib/udev/70-quantis-qrng.rules`](contrib/udev/70-quantis-qrng.rules)
gives access to the `plugdev` group and to the user logged in at the machine.
It's only in the repository, not in the crate: download it, or create
`70-quantis-qrng.rules` with this line:

```text
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="0aba", ATTR{idProduct}=="0102", MODE="0660", GROUP="plugdev", TAG+="uaccess"
```

Then install it:

```sh
sudo install -m 0644 70-quantis-qrng.rules /etc/udev/rules.d/
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
qrng list                                   # attached devices and serial numbers
qrng probe                                  # info and registers of the device
qrng read -n 1M > random.bin                # 1 MiB of random bytes
qrng read -n 1M -o random.bin               # same, without the shell (better on Windows)
qrng read -n 64 --hex                       # 64 bytes as hex
qrng read -n 10M | ent                      # pipe to another program
qrng read -n 10 --integers --min 1 --max 6  # 10 dice rolls
qrng read -n 10 --floats                    # 10 floats from 0 to 1
qrng read -n 1M -m matrix.dat -o random.bin # without bias (see Randomness extraction)
qrng probe --serial 1234567890              # pick a device if more are attached
```

`-n` defaults to 1024, with suffixes `K`, `M` and `G` (powers of 1024), no
upper limit. With `--integers` and `--floats` it's the number of values, one
per line: integers from `--min` to `--max` (both included), floats from `--min`
(included) to `--max` (excluded).

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

`fill_bytes` returns `Ok` **only if the whole buffer has been filled**, on error
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

### `rand`

With the `rand_core` feature there are three `TryRng`, the `rand` RNGs that
can fail (the device can be unplugged):

- `QuantisUsb`: every call reads from the device, raw output.
- `QuantisRng`: a 16 KiB buffer for many small values, raw output.
- `QuantisCryptoRng`: `QuantisRng` with the randomness extraction (see below),
  also `TryCryptoRng`.

The `rand` API (`random_range`,`shuffle`, ...) needs an RNG that doesn't fail.
This is possible by using `UnwrapErr`, that panics on device errors:

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

`QuantisUsb` and `QuantisRng` are not `TryCryptoRng`, the raw output can have
a small bias: for private keys use `QuantisCryptoRng`.

### Randomness extraction

The raw output can have a small bias. `extractor::Matrix` removes it like
`QuantisExtensions` does in the original library: every block of 1024 bits becomes 768 bits, with a matrix
of 1024 x 768 bits. In `qrng read` it's `-m FILE`, `--matrix-size-in` and
`--matrix-size-out`, like EasyQuantis.

```rust,no_run
use quantis_qrng::{extractor::Matrix, usb::QuantisUsb};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = std::fs::read("contrib/extractor/matrix.dat")?;
    let matrix = Matrix::new(&matrix, 1024, 768)?;

    // 2 blocks of 128 bytes -> 2 blocks of 96 bytes
    let mut raw = [0u8; 256];
    QuantisUsb::open()?.fill_bytes(&mut raw)?;
    let mut random = [0u8; 192];
    matrix.extract(&raw, &mut random);
    Ok(())
}
```

The matrix doesn't need to be secret, only independent from the data. The file
has `size_in * size_out / 8` bytes (98304 for 1024 x 768).

[`contrib/extractor/matrix.dat`](contrib/extractor/matrix.dat) is SHAKE256 of
`quantis-qrng`, as big as `default_idq_matrix.dat` (up to 4096 x 3840). It's
only in the repository, not in the crate: download it, or generate it again:

```sh
printf quantis-qrng | openssl shake256 -xoflen 1966080 -binary > matrix.dat
sha256sum matrix.dat   # c814fc25339597e5934a069eb79ccb469835c465b742596b0d2500532a0201e0
```

For your own matrix, take the bytes from another source, e.g.
`head -c 98304 /dev/urandom > matrix.dat` executed on a different computer.

With the `rand_core` feature, `QuantisCryptoRng` extracts the data and is a
`TryCryptoRng`. `UnwrapErr` gives a `CryptoRng` for the APIs that need one.
In the example below, a Bitcoin seed (BIP39, 24 words) with the [`bip39`](https://docs.rs/bip39)
crate is created by XORing the Quantis entropy with the one of the operating system:

```rust,no_run
use quantis_qrng::{
    extractor::Matrix,
    usb::{QuantisCryptoRng, QuantisUsb},
};
use rand::{TryRng, rngs::SysRng};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = std::fs::read("contrib/extractor/matrix.dat")?;
    let matrix = Matrix::new(&matrix, 1024, 768)?;
    let mut rng = QuantisCryptoRng::new(QuantisUsb::open()?, matrix);

    // 256 bits -> 24 words
    let mut entropy = [0u8; 32];
    rng.try_fill_bytes(&mut entropy)?;

    // XOR with the OS entropy
    let mut os = [0u8; 32];
    SysRng.try_fill_bytes(&mut os)?;
    for (e, o) in entropy.iter_mut().zip(&os) {
        *e ^= o;
    }
    os.fill(0); // cleanup

    let mnemonic = bip39::Mnemonic::from_entropy(&entropy)?;
    entropy.fill(0); // cleanup

    println!("{mnemonic}");
    Ok(())
}
```

## Differences from the original library

Same behaviour of the ID Quantique C library (release 24.4.2), with some small differences.
There is no official repository, a copy of the sources is in
[sanddweller/QRNGPcieDriver](https://github.com/sanddweller/QRNGPcieDriver/tree/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src).

- After a short USB transfer
  [`QuantisUsbRead`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L564-L609)
  returns success with the buffer partially filled, and `EasyQuantis` pads the
  file with zeros. Here the buffer is always filled, or the read fails.
- The device is selected by serial number, not by index. The index check of
  [`QuantisUsbOpen`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L439)
  is off by one so it reads a portion of uninitialized memory.
- Scaled integers
  ([`QuantisReadScaledInt`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Quantis_C.c#L773-L806))
  are biased (negative samples are never discarded and `max - min + 1` can
  overflow) and floats
  ([`ConvertToFloat_01`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Conversion.c#L68-L73),
  used by `EasyQuantis -f`) can be `1.0`. `qrng read --integers` has no bias, `--floats` never returns
  `--max`.
- `qrng list` doesn't open the devices (`EasyQuantis -l` does), so it doesn't
  get in the way of a running service.

## Wishlist

In no particular order:

- **Quantis PCIe** with the ID Quantique kernel driver (`/dev/qrandomN`), by using `ioctl`.
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
