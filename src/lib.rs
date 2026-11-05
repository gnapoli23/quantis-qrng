//! Unofficial pure-Rust driver for ID Quantique **Quantis** quantum random
//! number generators.
//!
//! > **Status: early development.** The vendor commands of Quantis USB work;
//! > reading random data is not implemented yet. PCIe devices are not
//! > supported yet.
//!
//! The driver runs entirely in user space and talks to the device through
//! [`nusb`](https://docs.rs/nusb): no C library and no kernel module are
//! needed.
//!
//! ```no_run
//! use quantis_qrng::usb::QuantisUsb;
//!
//! # fn main() -> Result<(), quantis_qrng::Error> {
//! let mut quantis = QuantisUsb::open()?;
//! println!("serial number: {:?}", quantis.serial_number());
//! println!("data rate: {} bytes/s", quantis.modules_rate()?);
//! # Ok(())
//! # }
//! ```
//!
//! On Linux a non-root user needs a udev rule to access the device.
//!
//! The `qrng` command-line tool is built with the `cli` feature:
//! `cargo install quantis-qrng --features cli`.
//!
//! This crate is not affiliated with or endorsed by ID Quantique SA.
//! "Quantis" is a trademark of ID Quantique SA.

mod error;
pub mod usb;

pub use error::Error;
