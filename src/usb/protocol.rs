//! USB protocol of the Quantis USB.
//!
//! Quantis USB has no kernel driver. The operating system only exposes the
//! raw USB device (on Linux `/dev/bus/usb/BBB/DDD`) and the driver talks to it
//! from user space with two kinds of transfers:
//!
//! * control transfers on endpoint 0 for the commands, see [`VendorRequest`];
//! * bulk IN transfers on [`ENDPOINT_BULK_IN`] for the random data.
//!
//! The values come from the original ID Quantique C library,
//! [`QuantisUsb_Commands.h`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Commands.h)
//! and [`QuantisUsb_Unix.c`](https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c).

use core::time::Duration;
use std::fmt;

/// USB vendor ID.
///
/// It is the ID of Ellisys, the maker of the USB controller inside the
/// device, not of ID Quantique.
pub const VENDOR_ID: u16 = 0x0ABA;

/// USB product ID.
pub const PRODUCT_ID: u16 = 0x0102;

/// Configuration selected when the device is opened (`bConfigurationValue`).
pub const CONFIGURATION: u8 = 1;

/// Interface claimed when the device is opened (`bInterfaceNumber`).
pub const INTERFACE: u8 = 0;

/// Address of the bulk endpoint of the random data.
///
/// Bit 7 is the direction (1 = IN, device to host) and bits 0..=3 are the
/// endpoint number, so it is IN endpoint 6. The device has no other data
/// endpoint.
pub const ENDPOINT_BULK_IN: u8 = 0x86;

/// Max bulk packet size on a high-speed (USB 2.0) port, in bytes.
///
/// On a full-speed (USB 1.1) port the packets are 64 bytes.
pub const MAX_BULK_PACKET_SIZE: usize = 512;

/// Timeout of every transfer (same as the original library).
pub(crate) const TRANSFER_TIMEOUT: Duration = Duration::from_millis(1000);

// fields of `bmRequestType`, the first byte of a control transfer
//
// | bit  | field     | values used here                        |
// |------|-----------|-----------------------------------------|
// | 7    | direction | 0 = host -> device, 1 = device -> host  |
// | 6..5 | type      | 0 = standard, 1 = class, 2 = vendor     |
// | 4..0 | recipient | 0 = device, 1 = interface, 2 = endpoint |
const DIRECTION_IN: u8 = 1 << 7;
const DIRECTION_OUT: u8 = 0 << 7;
const TYPE_VENDOR: u8 = 2 << 5;
const RECIPIENT_INTERFACE: u8 = 1;

/// `bmRequestType` of a vendor request that reads data (`0xC1`).
pub const REQUEST_TYPE_VENDOR_IN: u8 = DIRECTION_IN | TYPE_VENDOR | RECIPIENT_INTERFACE;

/// `bmRequestType` of a vendor request without data (`0x41`).
pub const REQUEST_TYPE_VENDOR_OUT: u8 = DIRECTION_OUT | TYPE_VENDOR | RECIPIENT_INTERFACE;

/// Vendor requests of the device.
///
/// They are sent as `bRequest` of a control transfer, with `wValue = 0` and
/// `wIndex = 0`. The enum is `#[non_exhaustive]`, so new requests can be added
/// in a minor release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum VendorRequest {
    /// Read the board version (4 bytes, little-endian).
    GetBoardVersion = 0x10,
    /// Disable the random number module (no data).
    ModuleDisable = 0x11,
    /// Enable the random number module (no data).
    ModuleEnable = 0x12,
    /// Read the module status (1 byte, not `0` if the module works).
    GetModulesStatus = 0x13,
    /// Read the module power state (1 byte).
    GetModulesPower = 0x14,
    /// Read the mask of the modules on the board (1 byte).
    GetModulesMask = 0x15,
    /// Read the module data rate in kbit/s (4 bytes, little-endian).
    GetModulesRate = 0x16,
    /// Read the AIS 31 start-up tests request flag (1 byte).
    GetAis31StartupTestsRequestFlag = 0x17,
    /// Clear the AIS 31 start-up tests request flag (no data).
    ClearAis31StartupTestsRequestFlag = 0x18,
}

impl fmt::Display for VendorRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VendorRequest::GetBoardVersion => write!(f, "GetBoardVersion ({0:#04x})", *self as u8),
            VendorRequest::ModuleDisable => write!(f, "ModuleDisable ({0:#04x})", *self as u8),
            VendorRequest::ModuleEnable => write!(f, "ModuleEnable ({0:#04x})", *self as u8),
            VendorRequest::GetModulesStatus => {
                write!(f, "GetModulesStatus ({0:#04x})", *self as u8)
            }
            VendorRequest::GetModulesPower => write!(f, "GetModulesPower ({0:#04x})", *self as u8),
            VendorRequest::GetModulesMask => write!(f, "GetModulesMask ({0:#04x})", *self as u8),
            VendorRequest::GetModulesRate => write!(f, "GetModulesRate ({0:#04x})", *self as u8),
            VendorRequest::GetAis31StartupTestsRequestFlag => {
                write!(f, "GetAis31StartupTestsRequestFlag ({0:#04x})", *self as u8)
            }
            VendorRequest::ClearAis31StartupTestsRequestFlag => {
                write!(
                    f,
                    "ClearAis31StartupTestsRequestFlag ({0:#04x})",
                    *self as u8
                )
            }
        }
    }
}

impl VendorRequest {
    /// Length of the response to this request, in bytes.
    ///
    /// It is `0` for the commands that send no data back.
    #[must_use]
    pub const fn response_len(self) -> usize {
        match self {
            Self::GetBoardVersion | Self::GetModulesRate => 4,
            Self::GetModulesStatus
            | Self::GetModulesPower
            | Self::GetModulesMask
            | Self::GetAis31StartupTestsRequestFlag => 1,
            Self::ModuleDisable | Self::ModuleEnable | Self::ClearAis31StartupTestsRequestFlag => 0,
        }
    }

    /// `bmRequestType` of this request.
    #[must_use]
    pub const fn request_type(self) -> u8 {
        if self.response_len() == 0 {
            REQUEST_TYPE_VENDOR_OUT
        } else {
            REQUEST_TYPE_VENDOR_IN
        }
    }
}

impl TryFrom<u8> for VendorRequest {
    type Error = UnknownVendorRequest;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x10 => Ok(VendorRequest::GetBoardVersion),
            0x11 => Ok(VendorRequest::ModuleDisable),
            0x12 => Ok(VendorRequest::ModuleEnable),
            0x13 => Ok(VendorRequest::GetModulesStatus),
            0x14 => Ok(VendorRequest::GetModulesPower),
            0x15 => Ok(VendorRequest::GetModulesMask),
            0x16 => Ok(VendorRequest::GetModulesRate),
            0x17 => Ok(VendorRequest::GetAis31StartupTestsRequestFlag),
            0x18 => Ok(VendorRequest::ClearAis31StartupTestsRequestFlag),
            _ => Err(UnknownVendorRequest(value)),
        }
    }
}

/// Error of [`VendorRequest::try_from`] for an unknown request code.
#[derive(Debug, thiserror::Error, Clone, Copy, PartialEq, Eq)]
#[error("unknown vendor request: {0:#04x}")]
pub struct UnknownVendorRequest(pub u8);

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [VendorRequest; 9] = [
        VendorRequest::ModuleEnable,
        VendorRequest::ModuleDisable,
        VendorRequest::ClearAis31StartupTestsRequestFlag,
        VendorRequest::GetAis31StartupTestsRequestFlag,
        VendorRequest::GetBoardVersion,
        VendorRequest::GetModulesMask,
        VendorRequest::GetModulesPower,
        VendorRequest::GetModulesRate,
        VendorRequest::GetModulesStatus,
    ];

    #[test]
    fn test_request_types_match_libusb_flags() {
        // LIBUSB_REQUEST_TYPE_VENDOR | LIBUSB_RECIPIENT_INTERFACE | LIBUSB_ENDPOINT_IN/OUT
        assert_eq!(REQUEST_TYPE_VENDOR_IN, 0x40 | 0x01 | 0x80);
        assert_eq!(REQUEST_TYPE_VENDOR_OUT, 0x40 | 0x01);
    }

    #[test]
    fn test_bulk_endpoint_is_in_endpoint_6() {
        assert_eq!(ENDPOINT_BULK_IN & 0x80, 0x80);
        assert_eq!(ENDPOINT_BULK_IN & 0x0F, 6);
    }

    #[test]
    fn test_commands_without_payload_are_out_requests() {
        assert_eq!(VendorRequest::ModuleEnable.request_type(), 0x41);
        assert_eq!(VendorRequest::GetModulesStatus.request_type(), 0xC1);
        assert_eq!(VendorRequest::GetBoardVersion as u8, 0x10);
    }

    #[test]
    fn test_vendor_request_conversion() {
        for v in ALL {
            assert_eq!(VendorRequest::try_from(v as u8), Ok(v));
        }
    }
}
