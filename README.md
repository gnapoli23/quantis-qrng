# quantis-qrng

Unofficial pure-Rust driver for ID Quantique **Quantis** quantum random number
generators (QRNG).

> [!WARNING]
> Early development. The Quantis USB commands work on real hardware, reading
> random data is implemented but still has to be tested on a real device.
> PCIe is not supported yet. Don't use it in production.

## Goals

- **Quantis USB** support in pure Rust, without C toolchain, `libusb`, `cmake`
  or `bindgen`.
- **Quantis PCIe** support with the ID Quantique kernel driver (`/dev/qrandomN`).
- A safe API: a read fills the whole buffer with data from the device, or it
  returns an error.
- A `qrng` command-line tool.

## Installation

Library:

```sh
cargo add quantis-qrng
```

`qrng` command-line tool:

```sh
cargo install quantis-qrng --features cli
# or from git, before the first release
cargo install --git https://github.com/gnapoli23/quantis-qrng --features cli
```

MSRV is Rust 1.85.

### Platforms

| OS      | Status |
|---------|--------|
| Linux   | supported and tested |
| macOS   | should work (no driver needed), not tested yet |
| Windows | not tested yet, needs the WinUSB driver (see below) |

On Windows the crate can only open the device if it uses the **WinUSB**
driver, it can't use the ID Quantique driver. The driver can be replaced with
[Zadig](https://zadig.akeo.ie), but then the ID Quantique software doesn't
work anymore on that machine. Also, use `qrng read -o FILE` instead of `>`:
the shell can change binary data (Windows PowerShell does).

### Device access on Linux

Quantis USB has no kernel driver, the program opens the USB device node
directly and by default only root can do it. The udev rule in
[`contrib/udev`](contrib/udev/70-quantis-qrng.rules) gives access to the
`plugdev` group and to the user logged in at the machine:

```sh
sudo install -m 0644 contrib/udev/70-quantis-qrng.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=0aba
sudo usermod -aG plugdev "$USER"   # then log out and log in again
```

If it still doesn't work, unplug and plug the device again. `qrng list` shows
the device even without permissions, `qrng probe` fails with "permission
denied" until the rule is applied.

Only one process at a time can use the device. To find who is using it run
`fuser -v /dev/bus/usb/BBB/DDD` (bus and device numbers from
`lsusb -d 0aba:0102`).

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

`-n` defaults to 1024 bytes and accepts the suffixes `K`, `M` and `G` (powers
of 1024). There is no upper limit, the data is read and written in chunks.

`qrng read` doesn't write binary data to a terminal, use `--hex`, `-o FILE` or
redirect the output. On Windows it prints a warning if binary data goes to
stdout, because the shell can change it: use `-o FILE`. If the read fails the
output just stops (it is never padded) and the exit status is 1.

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

`fill_bytes` returns `Ok` only if the whole buffer has been filled with data
from the device, on error the buffer is set to zero. Like the original library,
it checks the module status before every transfer.

`QuantisUsb::list` returns the attached devices without opening them, so it
works even if another program is using them.

## Differences from the original library

The crate follows the behaviour of the ID Quantique C library (release 24.4.2),
but not its bugs (they are described in the code). ID Quantique has no public
repository, a copy of the original sources is in
[sanddweller/QRNGPcieDriver](https://github.com/sanddweller/QRNGPcieDriver/tree/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src).

- After a short USB transfer the original read
  ([`QuantisUsbRead`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L564-L609))
  returns success with the buffer only partially filled, and `EasyQuantis`
  writes it to the file padded with zeros. Here the buffer is always filled, or
  the read fails.
- The device is selected by serial number and not by index. The original index
  check ([`QuantisUsbOpen`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L439))
  is off by one and reads uninitialized memory.
- `qrng list` doesn't open the devices (`EasyQuantis -l` does), so it can't
  get in the way of a running service.

## Wishlist

Things to do after 0.1 (Quantis USB, library and `qrng`), in no particular
order.

- **Standard Rust traits**
  - `std::io::Read` for the device, to use it with `io::copy`, `Read::take`,
    etc.
  - `From<Error> for std::io::Error`, with the right `io::ErrorKind` for
    timeout, permission and disconnection (but never `Interrupted`, that
    `io::copy` and `read_exact` silently retry).
- **`rand` support** with a `rand_core` feature (`rand_core` 0.10, `TryRng`).
  - A `QuantisRng` with an internal buffer, otherwise every `u32` costs a
    512 bytes USB packet. Buffered bytes are returned only once, the buffer is
    cleared on errors and on drop, no `Clone` or `Copy`.
  - No `TryCryptoRng` for the raw output, it can have a small bias. Better to
    use it as seed for a CSPRNG (`ChaCha20Rng::try_from_rng(&mut quantis_rng)`).
- **Uniform numbers**: integers in a range and floats in `[0, 1)`, in the
  library and in `qrng read` (`--integers --min --max`, `--floats`). The
  original library gets them wrong: scaled integers
  ([`QuantisReadScaledInt`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Quantis_C.c#L773-L806))
  are biased (negative samples are never discarded and `max - min + 1` can
  overflow) and doubles
  ([`ConvertToDouble_01`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/Conversion.c#L61-L66))
  can be `1.0`.
- **Randomness extraction**, the matrix extractor of `QuantisExtensions` that
  removes the bias of the raw output. It would allow a proper `TryCryptoRng`.
- **Quantis PCIe** with the ID Quantique kernel driver (`/dev/qrandomN`). Data
  is read with a normal `read()`, but the registers need `ioctl` and the crate
  forbids `unsafe` code. The original read loop
  ([`QuantisPciRead`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisPci_Unix.c#L463-L501))
  never ends if `read()` returns 0, to avoid here.
- **Public transport layer**, only if someone needs it (e.g. another USB
  backend). In 0.1 it is internal, so it can change without breaking anything,
  and it can be made public later.
- **More `qrng` commands**: enable/disable the module, clear the AIS 31 flag.
- **Other platforms**: test on macOS and Windows.

## Contributing

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org)
(`feat:`, `fix:`, `refactor:`, `docs:`, ..., with `!` for breaking changes).
[CHANGELOG.md](CHANGELOG.md) is generated from them with
[git-cliff](https://git-cliff.org): `git cliff -o CHANGELOG.md` (config in
`cliff.toml`).

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
