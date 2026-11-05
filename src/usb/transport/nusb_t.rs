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

/// ADD DOC HERE
#[derive(Debug)]
pub struct NusbTransport {
    interface: Interface,         // control_in and control_out
    endpoint: Endpoint<Bulk, In>, // bulk_in and max_packet_size
    serial_number: Option<String>,
    manufacturer: Option<String>,
}

impl NusbTransport {
    /// ADD DOC HERE
    ///
    /// # Errors
    /// ADD DOC HERE
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
        let dev_info = devices_info.remove(0); // safe

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
        #[allow(clippy::cast_possible_truncation)]
        let w_length = buf.len().min(u16::MAX as usize) as u16; // take u16 as the max
        let control_in = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Interface,
            request,
            value: 0,         // wValue -> 0 for custom vendor request
            index: 0,         // wIndex -> 0 for custom vendor request
            length: w_length, // wLength
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
            recipient: Recipient::Interface, // control, so `interface`
            request,
            value: 0,  // wValue -> 0 for custom vendor request
            index: 0,  // wIndex -> 0 for custom vendor request
            data: &[], // wLength -> no data
        };
        self.interface
            .control_out(control_out, TRANSFER_TIMEOUT)
            .wait()?;

        Ok(())
    }

    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, transport::TransportError> {
        // the buffer size  must be = n*max_size
        if buf.is_empty() || buf.len() % self.max_packet_size() != 0 {
            return Err(TransportError::InvalidArgument);
        }

        let buffer = Buffer::new(buf.len());
        let compl = self.endpoint.transfer_blocking(buffer, TRANSFER_TIMEOUT);

        if compl.status.is_ok() {
            let res = compl.into_result().unwrap(); // safe

            // partial completion?
            let n = res.len();
            if res.requested_len() > n {
                return Err(TransportError::InvalidArgument);
            }

            // return data
            buf[..n].copy_from_slice(&res);

            Ok(n)
        } else {
            // partial completion and timeout
            if compl.actual_len != 0 {
                return Err(TransportError::Timeout);
            }
            let res = compl.into_result().unwrap_err();

            Err(TransportError::from(res))
        }
    }

    fn max_packet_size(&self) -> usize {
        self.endpoint.max_packet_size()
    }
}
