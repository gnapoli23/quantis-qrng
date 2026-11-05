use crate::usb::{protocol::VendorRequest, transport::TransportError};

/// Error for communicating with a USB Quantis device.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport error
    #[error(transparent)]
    Transport(#[from] TransportError),

    /// Input/output error
    #[error("io error")]
    Io,

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

    /// Permission denied
    #[error("unable to execute the operation: permission denied")]
    PermissionDenied,

    /// Device is busy
    #[error("device is busy")]
    Busy,

    /// Invalid argument
    #[error("invalid argument")]
    InvalidArgument,

    /// No device found.
    #[error("no device found")]
    NoDevice,

    /// Too many devices connected
    #[error("too many devices")]
    TooManyDevices,

    /// No configuration present on device
    #[error("no configuration present on device")]
    NoConfiguration,

    /// Unexpected descriptor
    #[error("unexpected descriptor")]
    UnexpectedDescriptor,

    /// Any other failure reported by the USB stack.
    #[error("USB transfer failed")]
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
