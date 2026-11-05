//! [`UsbTransport`] backed by the [`nusb`] crate.

use nusb::{
    Endpoint, Interface, MaybeFuture as _, list_devices,
    transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Recipient},
};

use crate::{
    error::Error,
    usb::{
        protocol::{self, CONFIGURATION, ENDPOINT_BULK_IN, INTERFACE, TRANSFER_TIMEOUT},
        transport::{self, TransportError, UsbTransport},
    },
};

impl From<nusb::transfer::TransferError> for TransportError {
    fn from(value: nusb::transfer::TransferError) -> Self {
        match value {
            nusb::transfer::TransferError::Cancelled => TransportError::Timeout,
            nusb::transfer::TransferError::Stall => TransportError::Stall,
            nusb::transfer::TransferError::Disconnected => TransportError::Disconnected,
            nusb::transfer::TransferError::InvalidArgument => TransportError::InvalidArgument,
            nusb::transfer::TransferError::Fault | nusb::transfer::TransferError::Unknown(_) => {
                TransportError::Other(Box::new(value))
            }
        }
    }
}

/// A [`UsbTransport`] backed by the [`nusb`] crate: the transport used by
/// the real device.
///
/// It owns the claimed interface and the bulk IN endpoint, so the device stays
/// open, and unavailable to other processes, until the value is dropped.
#[derive(Debug)]
pub struct NusbTransport {
    interface: Interface,         // control_in and control_out
    endpoint: Endpoint<Bulk, In>, // bulk_in and max_packet_size
    serial_number: Option<String>,
    manufacturer: Option<String>,
}

impl NusbTransport {
    /// Finds the Quantis USB device, opens it, selects its configuration and
    /// claims its interface, like the original library.
    ///
    /// The serial number and the manufacturer are read here, from what the
    /// operating system cached at enumeration: no request is sent for them.
    ///
    /// # Errors
    ///
    /// * [`Error::NoDevice`] if no Quantis USB is attached.
    /// * [`Error::TooManyDevices`] if more than one is attached.
    /// * [`Error::PermissionDenied`] if the user cannot open the device (on
    ///   Linux, a udev rule is needed for non-root users).
    /// * [`Error::Busy`] if another process already holds the interface.
    /// * [`Error::UnexpectedDescriptor`] or [`Error::NoConfiguration`] if the
    ///   device does not look like a Quantis USB.
    /// * Any other error reported by the USB stack.
    pub fn open() -> Result<Self, Error> {
        // get devices
        let mut devices_info = list_devices()
            .wait()?
            .filter(|d| {
                d.vendor_id() == protocol::VENDOR_ID && d.product_id() == protocol::PRODUCT_ID
            })
            .collect::<Vec<_>>();

        if devices_info.is_empty() {
            return Err(Error::NoDevice);
        }

        if devices_info.len() > 1 {
            return Err(Error::TooManyDevices);
        }

        // device info
        let dev_info = devices_info.remove(0); // exactly one element, checked above

        // open device
        let device = dev_info.open().wait()?;

        // we expect to have a single configuration and a single interface
        if device.configurations().count() != 1 {
            return Err(Error::UnexpectedDescriptor);
        }
        device.set_configuration(CONFIGURATION).wait()?;

        let configuration = device
            .active_configuration()
            .map_err(|_| Error::NoConfiguration)?;
        if configuration.num_interfaces() != 1 {
            return Err(Error::UnexpectedDescriptor);
        }

        let interface = device.claim_interface(INTERFACE).wait()?;

        // bulk in endpoint
        let endpoint = interface.endpoint::<Bulk, In>(ENDPOINT_BULK_IN)?;

        Ok(NusbTransport {
            interface,
            endpoint,
            serial_number: dev_info.serial_number().map(String::from),
            manufacturer: dev_info.manufacturer_string().map(String::from),
        })
    }

    /// Serial number of the device, if the operating system provides it.
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// Manufacturer of the device, if the operating system provides it.
    ///
    /// Always `None` on Windows, which does not cache this string.
    #[must_use]
    pub fn manufacturer(&self) -> Option<&str> {
        self.manufacturer.as_deref()
    }
}

impl UsbTransport for NusbTransport {
    fn control_in(
        &mut self,
        request: u8,
        buf: &mut [u8],
    ) -> Result<usize, transport::TransportError> {
        let w_length = u16::try_from(buf.len()).map_err(|_| TransportError::InvalidArgument)?;
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request,
            value: 0,                    // wValue: unused by the vendor requests
            index: u16::from(INTERFACE), // wIndex: interface number (recipient = interface)
            length: w_length,            // wLength
        };
        let data = self
            .interface
            .control_in(control_in, TRANSFER_TIMEOUT)
            .wait()?;

        let n = data.len().min(w_length as usize);
        buf[..n].copy_from_slice(&data[..n]);

        Ok(n)
    }

    fn control_out(&mut self, request: u8) -> Result<(), transport::TransportError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request,
            value: 0,                    // wValue: unused by the vendor requests
            index: u16::from(INTERFACE), // wIndex: interface number (recipient = interface)
            data: &[],                   // wLength = 0: no data stage
        };
        self.interface
            .control_out(control_out, TRANSFER_TIMEOUT)
            .wait()?;

        Ok(())
    }

    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, transport::TransportError> {
        // the device only accepts requests for whole packets
        if buf.is_empty() || buf.len() % self.max_packet_size() != 0 {
            return Err(TransportError::InvalidArgument);
        }

        let buffer = Buffer::new(buf.len());
        let compl = self.endpoint.transfer_blocking(buffer, TRANSFER_TIMEOUT);

        let res = compl.into_result()?;
        let n = res.len();
        // a short packet ends the transfer early: only buf[..n] is filled
        buf[..n].copy_from_slice(&res);

        Ok(n)
    }

    fn max_packet_size(&self) -> usize {
        self.endpoint.max_packet_size()
    }
}
