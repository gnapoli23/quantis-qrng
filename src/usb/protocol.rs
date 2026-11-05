//! Wire protocol of the **Quantis USB** device.
//!
//! Quantis USB has no kernel driver: the operating system only exposes the raw
//! USB device (on Linux through `usbfs`, `/dev/bus/usb/BBB/DDD`) and the
//! "driver" is user-space code that talks to it with two kinds of transfers:
//!
//! * **Control transfers** on endpoint 0, used for commands such as "read the
//!   module status" or "enable the module". See [`VendorRequest`].
//! * **Bulk IN transfers** on [`ENDPOINT_BULK_IN`], used to stream random
//!   bytes from the device to the host.
//!
//! The values in this module come from the original ID Quantique C library
//! (`QuantisUsb_Commands.h` and `QuantisUsb_Unix.c`).

use core::time::Duration;
use std::fmt;

/// USB vendor ID of the device.
///
/// It belongs to Ellisys, the maker of the USB controller used inside the
/// Quantis, not to ID Quantique.
pub const VENDOR_ID: u16 = 0x0ABA;

/// USB product ID of the Quantis USB.
pub const PRODUCT_ID: u16 = 0x0102;

/// The configuration the driver selects (`bConfigurationValue`).
pub const CONFIGURATION: u8 = 1;

/// The interface the driver claims (`bInterfaceNumber`).
pub const INTERFACE: u8 = 0;

/// Address of the bulk endpoint that streams random data.
///
/// Bit 7 set means direction IN (device to host), bits 0..=3 are the endpoint
/// number, so this is IN endpoint 6.
/// The device has a single data endpoint, in direction IN (device to host).
pub const ENDPOINT_BULK_IN: u8 = 0x86;

/// Maximum size in bytes of a bulk IN packet
pub const MAX_BULK_PACKET_SIZE: usize = 512;

/// Timeout used by the original library for every transfer.
pub const TRANSFER_TIMEOUT: Duration = Duration::from_millis(1000);

// Fields of `bmRequestType`, the first byte of every control transfer:
//
// | bit  | field     | values used here                        |
// |------|-----------|-----------------------------------------|
// | 7    | direction | 0 = host -> device, 1 = device -> host    |
// | 6..5 | type      | 0 = standard, 1 = class, 2 = vendor     |
// | 4..0 | recipient | 0 = device, 1 = interface, 2 = endpoint |
const DIRECTION_IN: u8 = 1 << 7;
const DIRECTION_OUT: u8 = 0 << 7;
const TYPE_VENDOR: u8 = 2 << 5;
const RECIPIENT_INTERFACE: u8 = 1;

/// `bmRequestType` of a vendor request that reads data from the device
/// (`0xC1`).
pub const REQUEST_TYPE_VENDOR_IN: u8 = DIRECTION_IN | TYPE_VENDOR | RECIPIENT_INTERFACE;

/// `bmRequestType` of a vendor request that sends a command with no data
/// (`0x41`).
pub const REQUEST_TYPE_VENDOR_OUT: u8 = DIRECTION_OUT | TYPE_VENDOR | RECIPIENT_INTERFACE;

/// Vendor-specific commands, sent as the `bRequest` field of a control
/// transfer with `wValue = 0` and `wIndex = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VendorRequest {
    /// Read the board version (4 bytes, little-endian).
    GetBoardVersion = 0x10,
    /// Disable the random number module (no data).
    ModuleDisable = 0x11,
    /// Enable the random number module (no data).
    ModuleEnable = 0x12,
    /// Read the module status (1 byte, non-zero when the module works).
    GetModulesStatus = 0x13,
    /// Read the module power state (1 byte).
    GetModulesPower = 0x14,
    /// Read the mask of present modules (1 byte).
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
    /// Number of bytes the device returns for this request, or `0` for
    /// commands that only send a request (direction host -> device).
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

    /// The `bmRequestType` to use for this request.
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

/// Error for unknown vendor requests
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
