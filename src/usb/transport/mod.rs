//! USB transfers: nusb for the real device, a fake one for the tests.

/// An opened Quantis USB, ready for transfers.
pub(crate) trait UsbTransport {
    /// Vendor request with response, returns the bytes received.
    fn control_in(&mut self, request: u8, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// Vendor request without data.
    fn control_out(&mut self, request: u8) -> Result<(), TransportError>;

    /// Random data, `buf.len()` must be whole packets. Returns the bytes
    /// received (less after a short packet).
    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, TransportError>;

    /// `wMaxPacketSize` of the bulk IN endpoint.
    fn max_packet_size(&self) -> usize;
}

#[cfg(test)]
pub(crate) mod fake;

mod nusb_t;
pub(crate) use nusb_t::NusbTransport;

/// Enum and not a type parameter -> `QuantisUsb` stays a plain type.
#[derive(Debug)]
pub(crate) enum Transport {
    /// Real device.
    Nusb(NusbTransport),
    /// Fake device of the tests.
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

/// Why a USB transfer failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The transfer timed out.
    #[error("USB transfer timed out")]
    Timeout,

    /// The device has been unplugged.
    #[error("USB device disconnected")]
    Disconnected,

    /// The device rejected the request (USB STALL).
    #[error("USB device rejected the request (stall)")]
    Stall,

    /// Invalid request (bug in this crate).
    #[error("invalid USB transfer request")]
    InvalidArgument,

    /// Any other USB error.
    #[error("USB transfer failed")]
    Other(#[source] Box<dyn std::error::Error + Send + Sync>),
}
