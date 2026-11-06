//! [`UsbTransport`] implemented with [`nusb`].
//!
//! The `nusb` errors are converted with private functions and not with `From`
//! impls, so `nusb` is not part of the public API and a new major version of
//! `nusb` is not a breaking change for us.

use nusb::{
    Endpoint, Interface, MaybeFuture as _, list_devices,
    transfer::{Buffer, Bulk, ControlIn, ControlOut, ControlType, In, Recipient},
};

use crate::{
    error::Error,
    usb::{
        discovery::{self, DeviceInfo},
        protocol::{self, CONFIGURATION, ENDPOINT_BULK_IN, INTERFACE, TRANSFER_TIMEOUT},
        transport::{self, TransportError, UsbTransport},
    },
};

// nusb transfer error -> TransportError
fn transfer_error(value: nusb::transfer::TransferError) -> TransportError {
    use nusb::transfer::TransferError;
    match value {
        // nusb cancels the transfer when the timeout expires
        TransferError::Cancelled => TransportError::Timeout,
        TransferError::Stall => TransportError::Stall,
        TransferError::Disconnected => TransportError::Disconnected,
        TransferError::InvalidArgument => TransportError::InvalidArgument,
        TransferError::Fault | TransferError::Unknown(_) => TransportError::Other(Box::new(value)),
    }
}

// nusb error while listing or opening the device -> Error
fn open_error(value: nusb::Error) -> Error {
    match value.kind() {
        nusb::ErrorKind::Disconnected => Error::Disconnected,
        nusb::ErrorKind::Busy => Error::Busy,
        nusb::ErrorKind::PermissionDenied => Error::PermissionDenied,
        nusb::ErrorKind::NotFound => Error::NoDevice,
        // `ErrorKind` is non exhaustive, new kinds end up here
        _ => Error::Other(Box::new(value)),
    }
}

/// [`UsbTransport`] of the real device, implemented with [`nusb`].
///
/// It owns the claimed interface and the bulk IN endpoint, so the device stays
/// open (and other processes can't use it) until the value is dropped.
#[derive(Debug)]
pub(crate) struct NusbTransport {
    interface: Interface,         // control_in and control_out
    endpoint: Endpoint<Bulk, In>, // bulk_in and max_packet_size
    serial_number: Option<String>,
    manufacturer: Option<String>,
}

impl NusbTransport {
    /// Returns the attached Quantis USB devices, without opening them.
    pub(crate) fn list() -> Result<Vec<DeviceInfo>, Error> {
        Ok(quantis_devices()?
            .iter()
            .map(|d| DeviceInfo {
                serial_number: d.serial_number().map(String::from),
                manufacturer: d.manufacturer_string().map(String::from),
                product: d.product_string().map(String::from),
                bus_id: d.bus_id().to_string(),
                device_address: d.device_address(),
            })
            .collect())
    }

    /// Opens the device, selects the configuration and claims the interface,
    /// like the original library.
    ///
    /// Without `serial` exactly one device must be attached, otherwise the
    /// device with that serial number is opened (see `select_device`).
    ///
    /// Serial number and manufacturer are taken from the operating system,
    /// no request is sent to the device for them. The errors are the ones of
    /// `QuantisUsb::open` and `QuantisUsb::open_serial`.
    pub(crate) fn open(serial: Option<&str>) -> Result<Self, Error> {
        let dev_info =
            discovery::select_device(quantis_devices()?, serial, nusb::DeviceInfo::serial_number)?;

        // open device
        let device = dev_info.open().wait().map_err(open_error)?;

        // we expect to have a single configuration and a single interface
        if device.configurations().count() != 1 {
            return Err(Error::UnexpectedDescriptor);
        }
        device
            .set_configuration(CONFIGURATION)
            .wait()
            .map_err(open_error)?;

        let configuration = device
            .active_configuration()
            .map_err(|_| Error::NoConfiguration)?;
        if configuration.num_interfaces() != 1 {
            return Err(Error::UnexpectedDescriptor);
        }

        let interface = device
            .claim_interface(INTERFACE)
            .wait()
            .map_err(open_error)?;

        // bulk in endpoint
        let endpoint = interface
            .endpoint::<Bulk, In>(ENDPOINT_BULK_IN)
            .map_err(open_error)?;

        Ok(NusbTransport {
            interface,
            endpoint,
            serial_number: dev_info.serial_number().map(String::from),
            manufacturer: dev_info.manufacturer_string().map(String::from),
        })
    }

    /// Serial number of the device, if the operating system provides it.
    #[must_use]
    pub(crate) fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// Manufacturer of the device, if the operating system provides it
    /// (always `None` on Windows).
    #[must_use]
    pub(crate) fn manufacturer(&self) -> Option<&str> {
        self.manufacturer.as_deref()
    }
}

// attached devices with the Quantis USB vendor and product ID
fn quantis_devices() -> Result<Vec<nusb::DeviceInfo>, Error> {
    Ok(list_devices()
        .wait()
        .map_err(open_error)?
        .filter(|d| d.vendor_id() == protocol::VENDOR_ID && d.product_id() == protocol::PRODUCT_ID)
        .collect())
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
            value: 0,                    // wValue, not used by the vendor requests
            index: u16::from(INTERFACE), // wIndex, interface number (recipient = interface)
            length: w_length,            // wLength
        };
        let data = self
            .interface
            .control_in(control_in, TRANSFER_TIMEOUT)
            .wait()
            .map_err(transfer_error)?;

        let n = data.len().min(w_length as usize);
        buf[..n].copy_from_slice(&data[..n]);

        Ok(n)
    }

    fn control_out(&mut self, request: u8) -> Result<(), transport::TransportError> {
        let control_out = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request,
            value: 0,                    // wValue, not used by the vendor requests
            index: u16::from(INTERFACE), // wIndex, interface number (recipient = interface)
            data: &[],                   // wLength = 0, no data stage
        };
        self.interface
            .control_out(control_out, TRANSFER_TIMEOUT)
            .wait()
            .map_err(transfer_error)?;

        Ok(())
    }

    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, transport::TransportError> {
        // the device only accepts requests for whole packets
        if buf.is_empty() || buf.len() % self.max_packet_size() != 0 {
            return Err(TransportError::InvalidArgument);
        }

        let buffer = Buffer::new(buf.len());
        let compl = self.endpoint.transfer_blocking(buffer, TRANSFER_TIMEOUT);

        let res = compl.into_result().map_err(transfer_error)?;
        let n = res.len();
        if n > buf.len() {
            // can't happen with nusb, but better an error than a panic
            return Err(TransportError::Other(
                format!("bulk transfer returned {n} bytes, {} requested", buf.len()).into(),
            ));
        }
        // a short packet ends the transfer -> only buf[..n] is filled
        buf[..n].copy_from_slice(&res);

        Ok(n)
    }

    fn max_packet_size(&self) -> usize {
        self.endpoint.max_packet_size()
    }
}
