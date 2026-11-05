use crate::{
    error::Error,
    usb::{
        protocol::VendorRequest,
        transport::{NusbTransport, UsbTransport},
    },
};

pub mod protocol;
pub mod transport;

/// Implementation of a USB Quantis device.
/// It exposes all the available APIs the QRNG device supports.
pub struct QuantisUsb<T: UsbTransport = NusbTransport> {
    transport: T,
}

impl<T: UsbTransport> QuantisUsb<T> {
    fn read_exact<const N: usize>(&mut self, req: VendorRequest) -> Result<[u8; N], Error> {
        debug_assert_eq!(req.response_len(), N, "wrong response length for {req}");

        let mut buf = [0u8; N];
        let read = self.transport.control_in(req as u8, &mut buf)?;
        if read != N {
            return Err(Error::UnexpectedResponse {
                request: req,
                expected: N,
                received: read,
            });
        }
        Ok(buf)
    }

    // utility function for reading a u32 value
    // u32 values coming from the device are LE encoded
    fn read_word_value(&mut self, request: VendorRequest) -> Result<u32, Error> {
        let res = self.read_exact::<4>(request)?;
        let res = u32::from_le_bytes(res);
        Ok(res)
    }

    // utility function for reading a u8 value
    fn read_byte_value(&mut self, request: VendorRequest) -> Result<u8, Error> {
        let res = self.read_exact::<1>(request)?;
        let res = u8::from_be_bytes(res);

        Ok(res)
    }

    // utility function for writing some data
    fn send_command(&mut self, request: VendorRequest) -> Result<(), Error> {
        self.transport.control_out(request as u8)?;
        Ok(())
    }

    /// Create a new [`QuantisUsb`] instance with the specified [`UsbTransport`].
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Get the board version.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn board_version(&mut self) -> Result<u32, Error> {
        self.read_word_value(VendorRequest::GetBoardVersion)
    }

    /// Get the module mask.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn module_mask(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesMask)
    }

    /// Get the modules rate in bytes/s.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn modules_rate(&mut self) -> Result<u64, Error> {
        let modules_rate = self.read_word_value(VendorRequest::GetModulesRate)?;
        let modules_rate = u64::from(modules_rate) * 125; // kbit/s to bytes/s -> 1000 / 8

        let modules_maks = self.module_mask()?; // if the mask is 0, the rate is 0

        Ok(modules_rate * u64::from(modules_maks.count_ones()))
    }

    /// Get the modules power status.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn modules_power(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesPower)
    }

    /// Get the modules status.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn modules_status(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesStatus)
    }

    /// Get the AIS-31 startup tests request flag value.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn ais31_startup_tests_request_flag(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetAis31StartupTestsRequestFlag)
    }

    /// Disables the modules.
    /// NOTE: Quantis USB has a single module.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn modules_disable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidArgument);
        }
        self.send_command(VendorRequest::ModuleDisable)
    }

    /// Enables the modules.
    /// NOTE: Quantis USB has a single module.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn modules_enable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidArgument);
        }
        self.send_command(VendorRequest::ModuleEnable)
    }

    /// Clears the AIS-31 startup tests request flag.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue with the device or when the data
    /// received does not match the expected size.
    pub fn clear_ais31_startup_tests_request_flag(&mut self) -> Result<(), Error> {
        self.send_command(VendorRequest::ClearAis31StartupTestsRequestFlag)
    }
}

impl QuantisUsb<NusbTransport> {
    /// Creates a new connection with a [`QuantisUsb`] device instance with the specified [`UsbTransport`] layer.
    /// If not specified, it creates a connection with [`NusbTransport`] layer as default.
    ///
    /// NOTE: the original driver allows to have multiple devices attached but then only one i selected.
    /// We limit the selection to a single one as well, but we enforce to have a single device plugged at the time.
    ///
    /// # Errors
    /// Goes into error when there is any communication issue
    pub fn open() -> Result<Self, Error> {
        Ok(Self::new(NusbTransport::open()?))
    }

    /// Serial number of the device, if the operating system provides it.
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        self.transport.serial_number()
    }

    /// Manufacturer of the device, if the operating system provides it.
    ///
    /// Always `None` on Windows, which does not cache this string.
    #[must_use]
    pub fn manufacturer(&self) -> Option<&str> {
        self.transport.manufacturer()
    }

    /// Returns the max packet size for the connected device.
    #[must_use]
    pub fn max_packet_size(&self) -> usize {
        self.transport.max_packet_size()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::Error,
        usb::{QuantisUsb, protocol::VendorRequest, transport::fake::FakeUsbTransport},
    };

    #[test]
    fn test_get_board_version() {
        let expected = 0x1234_5678; // BE
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        let res = quantis.board_version().unwrap();
        assert_eq!(res, expected);
    }

    #[test]
    fn test_get_modules_rate() {
        let expected = 500_000;
        let mut transport = FakeUsbTransport::default();
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        transport.state.mask = 0b11;
        let expected = 1_000_000;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        transport.state.mask = 0;
        let expected = 0;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        // overflow
        transport.state.rate = u32::MAX;
        transport.state.mask = 0xFF;
        let mut quantis = QuantisUsb::new(transport);
        assert!(quantis.modules_rate().is_ok());
    }

    #[test]
    fn test_modules_disable() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 0);

        quantis.modules_enable(1).unwrap();
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 1);

        let res = quantis.modules_enable(0b10);
        assert!(res.is_err());
    }

    #[test]
    fn test_clear_ais31_flag() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        quantis.clear_ais31_startup_tests_request_flag().unwrap();
        let res = quantis.ais31_startup_tests_request_flag().unwrap();
        assert_eq!(res, 0);
    }

    #[test]
    fn test_unexpected_response() {
        let fake_transport = FakeUsbTransport::new(None, Some(2));
        let mut quantis = QuantisUsb::new(fake_transport);
        let res = quantis.board_version();
        assert!(matches!(
            res,
            Err(Error::UnexpectedResponse {
                request: VendorRequest::GetBoardVersion,
                expected: 4,
                received: 2
            })
        ));
    }
}
