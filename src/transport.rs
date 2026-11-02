//! Boundary between the driver logic and the USB stack.
//!
//! The driver logic (which commands to send, how to fill a buffer with random
//! bytes, how to decode responses) only talks to a [`UsbTransport`]. In
//! production the transport is backed by a real USB library; in tests it is a
//! simulated device, so the whole logic can be tested without hardware.

/// An opened Quantis USB device, ready to perform transfers.
///
/// Opening the device, selecting the configuration and claiming the interface
/// happen when the transport is created, so every method here operates on a
/// device that is already usable.
///
/// Control transfers are vendor requests addressed to the interface, with
/// `wValue = 0` and `wIndex = 0`; the direction is given by the method.
pub trait UsbTransport {
    /// Sends the vendor request `request` and reads its response into `buf`.
    ///
    /// The number of bytes requested from the device (`wLength`) is
    /// `buf.len()`, which must fit in a `u16`.
    ///
    /// Returns the number of bytes actually received, which can be smaller
    /// than `buf.len()`: only `buf[..n]` holds response data.
    ///
    /// # Errors
    ///
    /// Returns a [`TransportError`] if the transfer fails.
    fn control_in(&mut self, request: u8, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// Sends the vendor request `request`, with no data stage.
    ///
    /// # Errors
    ///
    /// Returns a [`TransportError`] if the transfer fails.
    fn control_out(&mut self, request: u8) -> Result<(), TransportError>;

    /// Reads random data from the bulk IN endpoint into `buf`.
    ///
    /// `buf.len()` must be a non-zero multiple of
    /// [`max_packet_size`](Self::max_packet_size): the device only accepts
    /// requests for whole packets.
    ///
    /// Returns the number of bytes actually received, which can be smaller
    /// than `buf.len()` when the device sends a short packet: only `buf[..n]`
    /// holds data from the device.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::InvalidArgument`] if `buf.len()` breaks the
    /// rule above, or another [`TransportError`] if the transfer fails.
    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// Maximum packet size of the bulk IN endpoint, in bytes (`wMaxPacketSize`).
    ///
    /// Typically 64 on a full-speed (USB 1.1) link and 512 on a high-speed
    /// (USB 2.0) link. It is read from the endpoint descriptor when the
    /// transport is created and does not change afterwards.
    fn max_packet_size(&self) -> usize;
}

/// Error returned by a [`UsbTransport`] operation.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The transfer did not complete within the transport's timeout.
    ///
    /// The device is still there: retrying can succeed.
    #[error("USB transfer timed out")]
    Timeout,

    /// The device was unplugged or is no longer reachable.
    ///
    /// Every further operation on this transport will fail; the device has to
    /// be opened again.
    #[error("USB device disconnected")]
    Disconnected,

    /// The device rejected the request (USB STALL), for example because it
    /// does not support the command.
    #[error("USB device rejected the request (stall)")]
    Stall,

    /// The request is invalid, for example a bulk read whose length is not a
    /// multiple of the maximum packet size.
    ///
    /// This points to a bug in the caller, not to a device problem.
    #[error("invalid USB transfer request")]
    InvalidArgument,

    /// Any other failure reported by the USB stack.
    #[error("USB transfer failed")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}
