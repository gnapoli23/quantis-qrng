//! Quantis USB devices.
//!
//! [`QuantisUsb`] is the device: it knows the vendor commands, how to decode
//! their responses and (soon) how to read random data. It talks to the
//! hardware only through a [`UsbTransport`], which by
//! default is [`NusbTransport`].
//!
//! * [`protocol`]: constants and commands of the USB wire protocol.
//! * [`transport`]: the boundary between the driver logic and the USB stack.

use crate::{
    error::Error,
    usb::{
        protocol::VendorRequest,
        transport::{NusbTransport, UsbTransport},
    },
};

pub mod protocol;
pub mod transport;

/// An opened Quantis USB device.
///
/// Use [`QuantisUsb::open`] to open the device attached to the system. The
/// type parameter `T` is the transport used to reach the device; it defaults
/// to [`NusbTransport`], so `QuantisUsb` alone is the real device.
///
/// Every operation takes `&mut self`: the device handles one request at a
/// time.
pub struct QuantisUsb<T: UsbTransport = NusbTransport> {
    transport: T,
}

impl<T: UsbTransport> QuantisUsb<T> {
    // Sends `req` and returns its response, which must be exactly `N` bytes long.
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

    // Reads a 4-byte value, which the device sends little-endian.
    fn read_word_value(&mut self, request: VendorRequest) -> Result<u32, Error> {
        let res = self.read_exact::<4>(request)?;
        let res = u32::from_le_bytes(res);
        Ok(res)
    }

    // Reads a 1-byte value.
    fn read_byte_value(&mut self, request: VendorRequest) -> Result<u8, Error> {
        let res = self.read_exact::<1>(request)?;
        let res = u8::from_be_bytes(res);
        Ok(res)
    }

    // Sends a command that carries no data.
    fn send_command(&mut self, request: VendorRequest) -> Result<(), Error> {
        self.transport.control_out(request as u8)?;
        Ok(())
    }

    /// Wraps an already opened transport.
    ///
    /// To open the real device use [`QuantisUsb::open`]; this constructor is
    /// for custom transports.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    /// Reads the board version.
    ///
    /// The value is opaque: the original library only displays it.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or if the response is not
    /// 4 bytes long.
    pub fn board_version(&mut self) -> Result<u32, Error> {
        self.read_word_value(VendorRequest::GetBoardVersion)
    }

    /// Reads the mask of the modules present on the board, one bit per module.
    ///
    /// Quantis USB has a single module, so the expected value is `1`.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or if the response is not
    /// 1 byte long.
    pub fn module_mask(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesMask)
    }

    /// Reads the data rate of the device, in bytes per second.
    ///
    /// The device reports the rate of one module in kbit/s; like the original
    /// library, this converts it to bytes per second and multiplies it by the
    /// number of modules in [`module_mask`](Self::module_mask). A mask of `0`
    /// gives a rate of `0`. A Quantis USB 4M reports 500 000 bytes/s.
    ///
    /// This sends two requests: the rate and the module mask.
    ///
    /// # Errors
    ///
    /// Returns an error if one of the transfers fails or if a response has the
    /// wrong length.
    pub fn modules_rate(&mut self) -> Result<u64, Error> {
        let modules_rate = self.read_word_value(VendorRequest::GetModulesRate)?;
        let modules_rate = u64::from(modules_rate) * 125; // kbit/s to bytes/s -> 1000 / 8

        let modules_maks = self.module_mask()?; // if the mask is 0, the rate is 0

        Ok(modules_rate * u64::from(modules_maks.count_ones()))
    }

    /// Reads the power state of the modules (`1` when powered).
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or if the response is not
    /// 1 byte long.
    pub fn modules_power(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesPower)
    }

    /// Reads the status of the modules.
    ///
    /// A non-zero value means the module is enabled and producing random data;
    /// `0` means it is disabled or not ready.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or if the response is not
    /// 1 byte long.
    pub fn modules_status(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetModulesStatus)
    }

    /// Reads the AIS 31 start-up tests request flag.
    ///
    /// A non-zero value means the device asks for the AIS 31 start-up tests to
    /// be run; clear it with
    /// [`clear_ais31_startup_tests_request_flag`](Self::clear_ais31_startup_tests_request_flag).
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or if the response is not
    /// 1 byte long.
    pub fn ais31_startup_tests_request_flag(&mut self) -> Result<u8, Error> {
        self.read_byte_value(VendorRequest::GetAis31StartupTestsRequestFlag)
    }

    /// Disables the random number module.
    ///
    /// `module_mask` selects the modules, one bit per module. Quantis USB has a
    /// single module, so bit 0 must be set; the other bits are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidModuleMask`] if bit 0 of `module_mask` is not
    /// set (no request is sent), or an error if the transfer fails.
    pub fn modules_disable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidModuleMask(module_mask));
        }
        self.send_command(VendorRequest::ModuleDisable)
    }

    /// Enables the random number module.
    ///
    /// `module_mask` selects the modules, one bit per module. Quantis USB has a
    /// single module, so bit 0 must be set; the other bits are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidModuleMask`] if bit 0 of `module_mask` is not
    /// set (no request is sent), or an error if the transfer fails.
    pub fn modules_enable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidModuleMask(module_mask));
        }
        self.send_command(VendorRequest::ModuleEnable)
    }

    /// Clears the AIS 31 start-up tests request flag.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails.
    pub fn clear_ais31_startup_tests_request_flag(&mut self) -> Result<(), Error> {
        self.send_command(VendorRequest::ClearAis31StartupTestsRequestFlag)
    }

    /// Maximum packet size of the bulk endpoint, in bytes.
    ///
    /// 512 when the device is attached to a high-speed (USB 2.0) port, 64 on a
    /// full-speed (USB 1.1) one.
    #[must_use]
    pub fn max_packet_size(&self) -> usize {
        self.transport.max_packet_size()
    }
}

impl QuantisUsb<NusbTransport> {
    /// Opens the Quantis USB device attached to the system.
    ///
    /// Exactly one device must be attached. The original library can select
    /// one among several devices by index; this crate does not support that
    /// yet and refuses to guess.
    ///
    /// Only one process at a time can open the device.
    ///
    /// # Errors
    ///
    /// See [`NusbTransport::open`].
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
}

#[cfg(test)]
mod tests {
    use crate::{
        error::Error,
        usb::{QuantisUsb, protocol::VendorRequest, transport::fake::FakeUsbTransport},
    };

    #[test]
    fn test_get_board_version() {
        let mut transport = FakeUsbTransport::default();
        transport.state.board_version = 0x1212_2121;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.board_version().unwrap();
        assert_eq!(res, 0x1212_2121);
    }

    #[test]
    fn test_get_modules_rate() {
        let expected = 500_000;
        let transport = FakeUsbTransport::default();
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        let mut transport = FakeUsbTransport::default();
        transport.state.mask = 0b11;
        let expected = 1_000_000;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        let mut transport = FakeUsbTransport::default();
        transport.state.mask = 0;
        let expected = 0;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        // overflow
        let mut transport = FakeUsbTransport::default();
        transport.state.rate = u32::MAX;
        transport.state.mask = 0xFF;
        let mut quantis = QuantisUsb::new(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, 4_294_967_295_000);
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

        // bit 0 not set: rejected before sending anything
        let err = quantis.modules_disable(0b10).unwrap_err();
        assert!(matches!(err, Error::InvalidModuleMask(0b10)));
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 1); // still enabled: the disable command was not sent

        let err = quantis.modules_enable(0b10).unwrap_err();
        assert!(matches!(err, Error::InvalidModuleMask(0b10)));
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
        let fake_transport = FakeUsbTransport::default().with_control_in_limit(2);
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
