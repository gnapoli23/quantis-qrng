//! Boundary between the driver logic and the USB stack.
//!
//! The driver logic (commands, reading random data, decoding the responses)
//! only talks to a [`UsbTransport`]. The real transport uses `nusb`, the tests
//! use a fake device, so the logic can be tested without hardware.
//!
//! This module is internal, only [`TransportError`] is public (re-exported in
//! `quantis_qrng::usb`) because it is part of [`Error`](crate::Error). With a
//! private trait we can add methods without breaking anyone, and `QuantisUsb`
//! always reads from a real device.

/// An opened Quantis USB device, ready to perform transfers.
///
/// The device is opened, configured and claimed when the transport is created,
/// so all the methods work on a device that is ready to use.
///
/// Control transfers are vendor requests to the interface, with `wValue = 0`
/// and `wIndex = 0`. The direction depends on the method.
pub(crate) trait UsbTransport {
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
    /// Usually 64 on a full-speed (USB 1.1) port and 512 on a high-speed
    /// (USB 2.0) port. It is read from the endpoint descriptor when the
    /// transport is created.
    fn max_packet_size(&self) -> usize;
}

#[cfg(test)]
pub(crate) mod fake;

mod nusb_t;
pub(crate) use nusb_t::NusbTransport;

/// Transport of a [`QuantisUsb`](crate::usb::QuantisUsb).
///
/// It is an enum and not a type parameter so `QuantisUsb` stays a normal type
/// in the public API. Outside the tests there is only the `Nusb` variant.
#[derive(Debug)]
pub(crate) enum Transport {
    /// Real device, with `nusb`.
    Nusb(NusbTransport),
    /// Fake device of the unit tests.
    #[cfg(test)]
    Fake(fake::FakeUsbTransport),
}

impl UsbTransport for Transport {
    fn control_in(&mut self, request: u8, buf: &mut [u8]) -> Result<usize, TransportError> {
        match self {
            Transport::Nusb(t) => t.control_in(request, buf),
            #[cfg(test)]
            Transport::Fake(t) => t.control_in(request, buf),
        }
    }

    fn control_out(&mut self, request: u8) -> Result<(), TransportError> {
        match self {
            Transport::Nusb(t) => t.control_out(request),
            #[cfg(test)]
            Transport::Fake(t) => t.control_out(request),
        }
    }

    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        match self {
            Transport::Nusb(t) => t.bulk_in(buf),
            #[cfg(test)]
            Transport::Fake(t) => t.bulk_in(buf),
        }
    }

    fn max_packet_size(&self) -> usize {
        match self {
            Transport::Nusb(t) => t.max_packet_size(),
            #[cfg(test)]
            Transport::Fake(t) => t.max_packet_size(),
        }
    }
}

/// Reason of a failed USB transfer, see [`Error::Transport`](crate::Error::Transport).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The transfer did not complete before the timeout.
    ///
    /// The device is still attached, so trying again can work.
    #[error("USB transfer timed out")]
    Timeout,

    /// The device has been unplugged.
    ///
    /// All the next operations fail too, the device must be opened again.
    #[error("USB device disconnected")]
    Disconnected,

    /// The device rejected the request (USB STALL), e.g. because it does not
    /// support the command.
    #[error("USB device rejected the request (stall)")]
    Stall,

    /// The request is not valid, e.g. a bulk read whose length is not a
    /// multiple of the max packet size.
    ///
    /// It means there is a bug in this crate, not a problem with the device.
    #[error("invalid USB transfer request")]
    InvalidArgument,

    /// Any other error of the USB stack.
    #[error("USB transfer failed")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}
