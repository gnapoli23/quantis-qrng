//! Error type of the crate.

use crate::usb::{protocol::VendorRequest, transport::TransportError};

/// Errors returned by this crate.
///
/// The enum is `#[non_exhaustive]`, new variants can be added in a minor
/// release (e.g. for PCI Express devices).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// A USB transfer failed, see [`TransportError`].
    #[error(transparent)]
    Transport(#[from] TransportError),

    /// The response to a vendor request has the wrong length.
    #[error("unexpected response to {request}: expected {expected} bytes, received {received}")]
    UnexpectedResponse {
        /// Request sent to the device.
        request: VendorRequest,
        /// Expected response length, in bytes.
        expected: usize,
        /// Received response length, in bytes.
        received: usize,
    },

    /// The module status is `0`, so the module is disabled or not ready.
    #[error("the random number module is disabled or not ready")]
    ModuleNotReady,

    /// A bulk transfer returned more bytes than requested.
    ///
    /// It should never happen: it means there is a bug in this crate or in
    /// the USB stack.
    #[error("bulk transfer returned {received} bytes, more than the {requested} requested")]
    BulkOverflow {
        /// Requested length, in bytes.
        requested: usize,
        /// Returned length, in bytes.
        received: usize,
    },

    /// A bulk transfer completed without any data.
    #[error("the device returned no random data")]
    NoData,

    /// The device has been unplugged.
    #[error("device disconnected")]
    Disconnected,

    /// The user has no permission to open the device.
    ///
    /// On Linux a udev rule is needed for non-root users.
    #[error("permission denied while opening the device")]
    PermissionDenied,

    /// The device is already in use, usually by another process.
    #[error("device is busy (already in use by another process?)")]
    Busy,

    /// The module mask does not have bit 0 set.
    #[error("invalid module mask: {0:#04x}")]
    InvalidModuleMask(u8),

    /// No Quantis device attached.
    #[error("no Quantis device found")]
    NoDevice,

    /// More than one device matches.
    ///
    /// Either more than one device is attached and no serial number was
    /// given, or two devices have the same serial number.
    #[error("more than one Quantis device found")]
    TooManyDevices,

    /// No attached device has the given serial number.
    #[error("no Quantis device with serial number {0:?}")]
    SerialNotFound(String),

    /// The device has no active configuration.
    #[error("no active configuration on the device")]
    NoConfiguration,

    /// The USB descriptors do not look like the ones of a Quantis USB.
    #[error("unexpected USB descriptors: not a Quantis USB?")]
    UnexpectedDescriptor,

    /// The extractor matrix has an invalid shape.
    #[error("invalid extractor matrix: {0}")]
    InvalidMatrix(String),

    /// The operating system can't do the operation on this device.
    ///
    /// On Windows it means that the device does not use the WinUSB driver,
    /// e.g. because the ID Quantique driver is installed.
    #[error("operation not supported by the device driver")]
    Unsupported(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Any other error of the USB stack while opening the device.
    #[error("USB operation failed")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        use std::io::ErrorKind;

        let kind = match &err {
            Error::Transport(TransportError::Timeout) => ErrorKind::TimedOut,
            Error::Transport(TransportError::Disconnected) | Error::Disconnected => {
                ErrorKind::NotConnected
            }
            Error::PermissionDenied => ErrorKind::PermissionDenied,
            Error::Busy => ErrorKind::ResourceBusy,
            Error::NoDevice | Error::SerialNotFound(_) => ErrorKind::NotFound,
            Error::Unsupported(_) => ErrorKind::Unsupported,
            Error::InvalidModuleMask(_) | Error::InvalidMatrix(_) => ErrorKind::InvalidInput,
            Error::UnexpectedResponse { .. } | Error::UnexpectedDescriptor => {
                ErrorKind::InvalidData
            }
            Error::Transport(
                TransportError::Stall | TransportError::InvalidArgument | TransportError::Other(_),
            )
            | Error::ModuleNotReady
            | Error::BulkOverflow { .. }
            | Error::NoData
            | Error::TooManyDevices
            | Error::NoConfiguration
            | Error::Other(_) => ErrorKind::Other,
        };
        std::io::Error::new(kind, err)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, ErrorKind};

    use crate::{
        error::Error,
        usb::{TransportError, protocol::VendorRequest},
    };

    #[test]
    fn test_io_error_kind() {
        let cases = [
            (
                Error::Transport(TransportError::Timeout),
                ErrorKind::TimedOut,
            ),
            (
                Error::Transport(TransportError::Disconnected),
                ErrorKind::NotConnected,
            ),
            (Error::Disconnected, ErrorKind::NotConnected),
            (Error::PermissionDenied, ErrorKind::PermissionDenied),
            (Error::Busy, ErrorKind::ResourceBusy),
            (Error::NoDevice, ErrorKind::NotFound),
            (Error::SerialNotFound("A".into()), ErrorKind::NotFound),
            (Error::Unsupported("winusb".into()), ErrorKind::Unsupported),
            (Error::InvalidModuleMask(0), ErrorKind::InvalidInput),
            (Error::InvalidMatrix("size".into()), ErrorKind::InvalidInput),
            (
                Error::UnexpectedResponse {
                    request: VendorRequest::GetBoardVersion,
                    expected: 4,
                    received: 2,
                },
                ErrorKind::InvalidData,
            ),
            (Error::UnexpectedDescriptor, ErrorKind::InvalidData),
            (Error::Transport(TransportError::Stall), ErrorKind::Other),
            (Error::ModuleNotReady, ErrorKind::Other),
            (Error::NoData, ErrorKind::Other),
            (Error::TooManyDevices, ErrorKind::Other),
        ];
        for (err, kind) in cases {
            let msg = err.to_string();
            assert_eq!(io::Error::from(err).kind(), kind, "{msg}");
        }
    }

    #[test]
    fn test_io_error_keeps_the_original() {
        let err = io::Error::from(Error::Busy);
        let inner = err.get_ref().and_then(|e| e.downcast_ref::<Error>());
        assert!(matches!(inner, Some(Error::Busy)));
    }
}
