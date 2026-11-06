//! Unofficial pure-Rust driver for ID Quantique Quantis quantum random number
//! generators (QRNG).
//!
//! The driver runs in user space and talks to the device with [`nusb`], so
//! there is no C library, `libusb` or kernel module to install.
//!
//! At the moment only **Quantis USB** is supported, through [`usb::QuantisUsb`].
//!
//! # Example
//!
//! ```no_run
//! use quantis_qrng::usb::QuantisUsb;
//!
//! # fn main() -> Result<(), quantis_qrng::Error> {
//! let mut quantis = QuantisUsb::open()?;
//! println!("serial number: {:?}", quantis.serial_number());
//! println!("data rate: {} bytes/s", quantis.modules_rate()?);
//!
//! let mut random = [0u8; 32];
//! quantis.fill_bytes(&mut random)?;
//! # Ok(())
//! # }
//! ```
//!
//! If more than one device is attached, pick one with
//! [`QuantisUsb::open_serial`](usb::QuantisUsb::open_serial). The serial
//! numbers are returned by [`QuantisUsb::list`](usb::QuantisUsb::list).
//!
//! # Device access
//!
//! On Linux only root can open the device by default. The repository ships a
//! udev rule (`contrib/udev/70-quantis-qrng.rules`) that gives access to the
//! `plugdev` group and to the user logged in at the machine.
//!
//! Only one process at a time can open a device.
//!
//! # Platforms
//!
//! Linux is supported and tested. macOS should work but it is not tested
//! yet. Windows is not tested yet, and the device must use the WinUSB driver
//! (the crate can't use the ID Quantique driver).
//!
//! # Command-line tool
//!
//! The `qrng` tool is built with the `cli` feature:
//!
//! ```sh
//! cargo install quantis-qrng --features cli
//! qrng list
//! qrng read -n 1M > random.bin
//! ```
//!
//! # Original library
//!
//! The crate is a port of the Quantis C library of ID Quantique (release
//! 24.4.2). ID Quantique distributes it as an archive, there is no official
//! repository, but a copy of the sources is on GitHub:
//! [`libs/src`](https://github.com/sanddweller/QRNGPcieDriver/tree/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src).
//! The library is in `Quantis/`, the `EasyQuantis` tool in `EasyQuantis/`.
//!
//! # Disclaimer
//!
//! This crate is not affiliated with or endorsed by ID Quantique SA.
//! "Quantis" is a trademark of ID Quantique SA.
//!
//! [`nusb`]: https://docs.rs/nusb

mod error;
pub mod usb;

pub use error::Error;

// README examples are compiled as doctests
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
