//! The error type of the crate.

use crate::usb::{protocol::VendorRequest, transport::TransportError};

/// Error returned by the operations of this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A USB transfer failed. See [`TransportError`] for the reason.
    #[error(transparent)]
    Transport(#[from] TransportError),

    /// The device answered a vendor request with a response of the wrong
    /// length.
    #[error("unexpected response to {request}: expected {expected} bytes, received {received}")]
    UnexpectedResponse {
        /// The request that was sent.
        request: VendorRequest,
        /// Number of bytes the request should return.
        expected: usize,
        /// Number of bytes actually received.
        received: usize,
    },

    /// The device was unplugged or is no longer reachable.
    #[error("device disconnected")]
    Disconnected,

    /// The user is not allowed to open the device.
    ///
    /// On Linux, non-root users need a udev rule that grants access to the
    /// device node.
    #[error("permission denied while opening the device")]
    PermissionDenied,

    /// The device is already in use, usually by another process.
    #[error("device is busy (already in use by another process?)")]
    Busy,

    /// The module mask does not select the module of the device (bit 0).
    #[error("invalid module mask: {0:#04x}")]
    InvalidModuleMask(u8),

    /// No Quantis device is attached.
    #[error("no Quantis device found")]
    NoDevice,

    /// More than one Quantis device is attached, and the operation needs
    /// exactly one.
    #[error("more than one Quantis device found")]
    TooManyDevices,

    /// The device has no active configuration.
    #[error("no active configuration on the device")]
    NoConfiguration,

    /// The device descriptors do not match a Quantis USB (number of
    /// configurations or interfaces).
    #[error("unexpected USB descriptors: not a Quantis USB?")]
    UnexpectedDescriptor,

    /// Any other failure reported by the USB stack while opening the device.
    #[error("USB operation failed")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<nusb::Error> for Error {
    fn from(value: nusb::Error) -> Self {
        match value.kind() {
            nusb::ErrorKind::Disconnected => Error::Disconnected,
            nusb::ErrorKind::Busy => Error::Busy,
            nusb::ErrorKind::PermissionDenied => Error::PermissionDenied,
            nusb::ErrorKind::NotFound => Error::NoDevice,
            // `ErrorKind` is `#[non_exhaustive]`: new kinds end up here
            _ => Error::Other(Box::new(value)),
        }
    }
}
