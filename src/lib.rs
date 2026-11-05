//! Unofficial pure-Rust driver for ID Quantique **Quantis** quantum random
//! number generators.
//!
//!
//! This crate is not affiliated with or endorsed by ID Quantique SA.
//! "Quantis" is a trademark of ID Quantique SA.

mod error;
mod usb;

pub use error::Error;
pub use usb::{QuantisUsb, transport};
