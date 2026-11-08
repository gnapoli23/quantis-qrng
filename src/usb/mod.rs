//! Quantis USB devices.
//!
//! [`QuantisUsb`] is the main type: it opens the device, reads random data and
//! sends the vendor commands. [`QuantisUsb::list`] returns the attached devices
//! as [`DeviceInfo`].
//!
//! The constants and the commands of the USB protocol are in [`protocol`].

use crate::{
    error::Error,
    usb::{
        protocol::VendorRequest,
        transport::{NusbTransport, Transport, UsbTransport},
    },
};

mod discovery;
pub mod protocol;
#[cfg(feature = "rand_core")]
mod rng;
pub(crate) mod transport;

pub use discovery::DeviceInfo;
#[cfg(feature = "rand_core")]
pub use rng::{QuantisCryptoRng, QuantisRng};
pub use transport::TransportError;

// max size of a single bulk request, in bytes
//
// the device sends ~500 000 bytes/s and every transfer has a 1 s timeout, so a
// request must be much smaller than that: 16 KiB take ~33 ms. it is also a
// multiple of both packet sizes (64 and 512)
const READ_CHUNK_SIZE: usize = 16 * 1024;

/// An opened Quantis USB device.
///
/// The device is claimed until the value is dropped, so no other process can
/// use it in the meantime.
///
/// All the methods take `&mut self` because the device handles one request at
/// a time. `QuantisUsb` is `Send`: to share it between threads put it in a
/// `Mutex`.
///
/// # Example
///
/// ```no_run
/// use quantis_qrng::usb::QuantisUsb;
///
/// # fn main() -> Result<(), quantis_qrng::Error> {
/// let mut quantis = QuantisUsb::open()?;
///
/// let mut buf = [0u8; 64];
/// quantis.fill_bytes(&mut buf)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct QuantisUsb {
    transport: Transport,
}

impl QuantisUsb {
    /// Returns the Quantis USB devices attached to the system.
    ///
    /// The devices are not opened, so this works even if they are in use by
    /// another process. If no device is attached the list is empty.
    ///
    /// # Errors
    ///
    /// Returns an error if the USB devices cannot be listed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use quantis_qrng::usb::QuantisUsb;
    ///
    /// # fn main() -> Result<(), quantis_qrng::Error> {
    /// for device in QuantisUsb::list()? {
    ///     println!("{:?}", device.serial_number());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn list() -> Result<Vec<DeviceInfo>, Error> {
        NusbTransport::list()
    }

    /// Opens the Quantis USB device attached to the system.
    ///
    /// Exactly one device must be attached. With more than one it returns
    /// [`Error::TooManyDevices`], use [`open_serial`](Self::open_serial)
    /// instead.
    ///
    /// Like the original library, it selects configuration 1 and claims
    /// interface 0. On Windows the configuration is selected by the system,
    /// and the device must use the WinUSB driver.
    ///
    /// # Errors
    ///
    /// * [`Error::NoDevice`] if no device is attached.
    /// * [`Error::TooManyDevices`] if more than one device is attached.
    /// * [`Error::PermissionDenied`] if the user cannot open the device.
    /// * [`Error::Busy`] if the device is already in use.
    /// * [`Error::UnexpectedDescriptor`] or [`Error::NoConfiguration`] if the
    ///   device does not look like a Quantis USB.
    /// * [`Error::Unsupported`] on Windows if the device does not use the
    ///   WinUSB driver.
    /// * [`Error::Other`] for any other USB error.
    pub fn open() -> Result<Self, Error> {
        Ok(Self {
            transport: Transport::Nusb(NusbTransport::open(None)?),
        })
    }

    /// Opens the device with the given serial number.
    ///
    /// The serial number must match exactly, see
    /// [`DeviceInfo::serial_number`]. The original library selects the device
    /// by index instead, but the index can change when a device is plugged in
    /// or out.
    ///
    /// # Errors
    ///
    /// * [`Error::SerialNotFound`] if no device has this serial number.
    /// * [`Error::TooManyDevices`] if two devices have this serial number.
    /// * The same errors of [`open`](Self::open).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use quantis_qrng::usb::QuantisUsb;
    ///
    /// # fn main() -> Result<(), quantis_qrng::Error> {
    /// let quantis = QuantisUsb::open_serial("1234567890")?;
    /// println!("opened {:?}", quantis.serial_number());
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_serial(serial: &str) -> Result<Self, Error> {
        Ok(Self {
            transport: Transport::Nusb(NusbTransport::open(Some(serial))?),
        })
    }

    /// Serial number of the device, if the operating system provides it.
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        match &self.transport {
            Transport::Nusb(t) => t.serial_number(),
            #[cfg(test)]
            Transport::Fake(_) => None,
        }
    }

    /// Manufacturer of the device, if the operating system provides it.
    ///
    /// It is always `None` on Windows.
    #[must_use]
    pub fn manufacturer(&self) -> Option<&str> {
        match &self.transport {
            Transport::Nusb(t) => t.manufacturer(),
            #[cfg(test)]
            Transport::Fake(_) => None,
        }
    }

    /// Max packet size of the bulk endpoint, in bytes.
    ///
    /// It is 512 on a high-speed (USB 2.0) port and 64 on a full-speed
    /// (USB 1.1) one.
    #[must_use]
    pub fn max_packet_size(&self) -> usize {
        self.transport.max_packet_size()
    }

    /// Fills `buf` with random bytes from the device.
    ///
    /// It returns `Ok(())` only if the whole buffer has been filled with data
    /// coming from the device. An empty buffer returns `Ok(())` without
    /// sending anything.
    ///
    /// If an error occurs, `buf` is set to zero, so a partially filled buffer
    /// can never be used by mistake. Don't use `buf` after an error.
    ///
    /// Like the original library, it checks the module status before every
    /// transfer and stops with [`Error::ModuleNotReady`] if the module is not
    /// working. The bytes that the device sends after the end of `buf` are
    /// thrown away, they are never returned by a later call.
    ///
    /// # Errors
    ///
    /// * [`Error::ModuleNotReady`] if the module status is `0`.
    /// * [`Error::NoData`] if a transfer returns no data.
    /// * [`Error::BulkOverflow`] if a transfer returns more data than
    ///   requested.
    /// * [`Error::UnexpectedDescriptor`] if the max packet size is `0`.
    /// * [`Error::Transport`] or [`Error::UnexpectedResponse`] if a transfer
    ///   fails, e.g. on timeout or if the device is unplugged.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use quantis_qrng::usb::QuantisUsb;
    ///
    /// # fn main() -> Result<(), quantis_qrng::Error> {
    /// let mut quantis = QuantisUsb::open()?;
    ///
    /// let mut seed = [0u8; 32];
    /// quantis.fill_bytes(&mut seed)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn fill_bytes(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let result = self.fill(buf);
        if result.is_err() {
            // no partial data left in the buffer
            buf.fill(0);
        }
        result
    }

    /// Reads the board version.
    ///
    /// The value has no documented meaning, the original library just prints
    /// it.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or the response is not 4 bytes
    /// long.
    pub fn board_version(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(
            self.read_response(VendorRequest::GetBoardVersion)?,
        ))
    }

    /// Reads the mask of the modules on the board, one bit per module.
    ///
    /// Quantis USB has a single module, so the value should be `1`.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or the response is not 1 byte
    /// long.
    pub fn module_mask(&mut self) -> Result<u8, Error> {
        Ok(u8::from_be_bytes(
            self.read_response(VendorRequest::GetModulesMask)?,
        ))
    }

    /// Reads the data rate of the device, in bytes per second.
    ///
    /// The device returns the rate of one module in kbit/s. Like the original
    /// library, it is converted to bytes per second and multiplied by the
    /// number of modules in [`module_mask`](Self::module_mask), so a mask of
    /// `0` gives `0`. A Quantis USB 4M returns 500 000 bytes/s.
    ///
    /// It sends two requests, one for the rate and one for the mask.
    ///
    /// # Errors
    ///
    /// Returns an error if a transfer fails or a response has the wrong
    /// length.
    pub fn modules_rate(&mut self) -> Result<u64, Error> {
        let modules_rate = u32::from_le_bytes(self.read_response(VendorRequest::GetModulesRate)?);
        let modules_rate = u64::from(modules_rate) * 125; // kbit/s to bytes/s -> 1000 / 8

        let modules_mask = self.module_mask()?; // if the mask is 0, the rate is 0

        Ok(modules_rate * u64::from(modules_mask.count_ones()))
    }

    /// Reads the power state of the modules, `1` if powered.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or the response is not 1 byte
    /// long.
    pub fn modules_power(&mut self) -> Result<u8, Error> {
        Ok(u8::from_be_bytes(
            self.read_response(VendorRequest::GetModulesPower)?,
        ))
    }

    /// Reads the status of the modules.
    ///
    /// A value other than `0` means that the module is enabled and is
    /// producing random data.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or the response is not 1 byte
    /// long.
    pub fn modules_status(&mut self) -> Result<u8, Error> {
        Ok(u8::from_be_bytes(
            self.read_response(VendorRequest::GetModulesStatus)?,
        ))
    }

    /// Reads the AIS 31 start-up tests request flag.
    ///
    /// A value other than `0` means that the device asks to run the AIS 31
    /// start-up tests. The flag is cleared with
    /// [`clear_ais31_startup_tests_request_flag`](Self::clear_ais31_startup_tests_request_flag).
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails or the response is not 1 byte
    /// long.
    pub fn ais31_startup_tests_request_flag(&mut self) -> Result<u8, Error> {
        Ok(u8::from_be_bytes(self.read_response(
            VendorRequest::GetAis31StartupTestsRequestFlag,
        )?))
    }

    /// Disables the random number module.
    ///
    /// `module_mask` has one bit per module. Quantis USB has a single module,
    /// so bit 0 must be set, the other bits are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidModuleMask`] if bit 0 is not set (nothing is
    /// sent to the device), or an error if the transfer fails.
    pub fn modules_disable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidModuleMask(module_mask));
        }
        Ok(self
            .transport
            .control_out(VendorRequest::ModuleDisable as u8)?)
    }

    /// Enables the random number module.
    ///
    /// `module_mask` has one bit per module. Quantis USB has a single module,
    /// so bit 0 must be set, the other bits are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidModuleMask`] if bit 0 is not set (nothing is
    /// sent to the device), or an error if the transfer fails.
    pub fn modules_enable(&mut self, module_mask: u8) -> Result<(), Error> {
        if (module_mask & 0x0001) != 0x0001 {
            // Quantis USB has a single module
            return Err(Error::InvalidModuleMask(module_mask));
        }
        Ok(self
            .transport
            .control_out(VendorRequest::ModuleEnable as u8)?)
    }

    /// Clears the AIS 31 start-up tests request flag.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer fails.
    pub fn clear_ais31_startup_tests_request_flag(&mut self) -> Result<(), Error> {
        Ok(self
            .transport
            .control_out(VendorRequest::ClearAis31StartupTestsRequestFlag as u8)?)
    }

    // body of `fill_bytes`, on error `buf` can be partially filled
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        if buf.is_empty() {
            return Ok(());
        }

        let packet_size = self.max_packet_size();
        if packet_size == 0 {
            return Err(Error::UnexpectedDescriptor);
        }
        // biggest request -> whole packets, at least one
        let max_chunk = (READ_CHUNK_SIZE / packet_size).max(1) * packet_size;

        // one packet for the tail, allocated only if needed
        let mut scratch: Vec<u8> = Vec::new();
        let mut filled = 0;

        while filled < buf.len() {
            if self.modules_status()? == 0 {
                return Err(Error::ModuleNotReady);
            }

            let remaining = buf.len() - filled;
            let added = if remaining >= packet_size {
                // whole packets, directly into `buf`
                let chunk = (remaining - remaining % packet_size).min(max_chunk);
                let n = self.transport.bulk_in(&mut buf[filled..filled + chunk])?;
                // short packet -> read again, the C library stops here without error
                // https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L597-L600
                if n == 0 {
                    return Err(Error::NoData);
                }
                if n > chunk {
                    return Err(Error::BulkOverflow {
                        requested: chunk,
                        received: n,
                    });
                }
                n
            } else {
                // tail -> one packet into `scratch`, keep only what fits
                if scratch.is_empty() {
                    scratch = vec![0u8; packet_size];
                }
                let n = self.transport.bulk_in(&mut scratch)?;
                // same checks as above
                if n == 0 {
                    return Err(Error::NoData);
                }
                if n > packet_size {
                    return Err(Error::BulkOverflow {
                        requested: packet_size,
                        received: n,
                    });
                }
                let take = n.min(remaining);
                buf[filled..filled + take].copy_from_slice(&scratch[..take]);
                take
            };

            // `added` is at least 1, so the loop always ends.
            // after a short packet `filled` is not a multiple of the packet size
            // anymore, that's ok: only the request length must be
            filled += added;
        }

        // clear the bytes we threw away (the compiler could still skip it)
        scratch.fill(0);
        std::hint::black_box(&scratch);

        Ok(())
    }

    // sends `req` and returns the response, that must be exactly `N` bytes long
    fn read_response<const N: usize>(&mut self, req: VendorRequest) -> Result<[u8; N], Error> {
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

    // only for tests: device backed by the fake transport
    #[cfg(test)]
    pub(crate) fn with_fake(fake: transport::fake::FakeUsbTransport) -> Self {
        Self {
            transport: Transport::Fake(fake),
        }
    }

    // only for tests: access to the fake, to check what it received
    #[cfg(test)]
    pub(crate) fn fake(&self) -> &transport::fake::FakeUsbTransport {
        match &self.transport {
            Transport::Fake(fake) => fake,
            Transport::Nusb(_) => panic!("not a fake device"),
        }
    }
}

/// The device never ends: `read` returns `Ok(0)` only if `buf` is empty,
/// `read_to_end` and `read_to_string` fail, use [`take`](std::io::Read::take).
///
/// `read` returns at most 16 KiB, in whole packets if `buf` is larger than a
/// packet. For many small reads use a [`BufReader`](std::io::BufReader).
///
/// `read_exact` works like [`fill_bytes`](QuantisUsb::fill_bytes).
///
/// # Example
///
/// ```no_run
/// use std::{fs::File, io::{self, Read}};
/// use quantis_qrng::usb::QuantisUsb;
///
/// fn main() -> io::Result<()> {
///     let quantis = QuantisUsb::open()?;
///
///     // 1 MiB to a file
///     let mut file = File::create("random.bin")?;
///     io::copy(&mut quantis.take(1024 * 1024), &mut file)?;
///     Ok(())
/// }
/// ```
impl std::io::Read for QuantisUsb {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let packet_size = self.max_packet_size();
        let len = if packet_size == 0 || buf.len() < packet_size {
            buf.len() // tail (packet size 0 -> error from `fill_bytes`)
        } else {
            // whole packets, nothing thrown away
            let len = buf.len().min(READ_CHUNK_SIZE.max(packet_size));
            len - len % packet_size
        };
        self.fill_bytes(&mut buf[..len])?;
        Ok(len)
    }

    // the default one leaves partial data in `buf` on error
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        Ok(self.fill_bytes(buf)?)
    }

    // the default ones would read until the memory is full
    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the device never ends, use `Read::take`",
        ))
    }

    fn read_to_string(&mut self, _buf: &mut String) -> std::io::Result<usize> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the device never ends, use `Read::take`",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use crate::{
        error::Error,
        usb::{
            QuantisUsb, READ_CHUNK_SIZE,
            protocol::VendorRequest,
            transport::{TransportError, fake::FakeUsbTransport},
        },
    };

    #[test]
    fn test_get_board_version() {
        let mut transport = FakeUsbTransport::default();
        transport.state.board_version = 0x1212_2121;
        let mut quantis = QuantisUsb::with_fake(transport);
        let res = quantis.board_version().unwrap();
        assert_eq!(res, 0x1212_2121);
    }

    #[test]
    fn test_get_modules_rate() {
        let expected = 500_000;
        let transport = FakeUsbTransport::default();
        let mut quantis = QuantisUsb::with_fake(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        let mut transport = FakeUsbTransport::default();
        transport.state.mask = 0b11;
        let expected = 1_000_000;
        let mut quantis = QuantisUsb::with_fake(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        let mut transport = FakeUsbTransport::default();
        transport.state.mask = 0;
        let expected = 0;
        let mut quantis = QuantisUsb::with_fake(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, expected);

        // overflow
        let mut transport = FakeUsbTransport::default();
        transport.state.rate = u32::MAX;
        transport.state.mask = 0xFF;
        let mut quantis = QuantisUsb::with_fake(transport);
        let res = quantis.modules_rate().unwrap();
        assert_eq!(res, 4_294_967_295_000);
    }

    #[test]
    fn test_modules_disable() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 0);

        quantis.modules_enable(1).unwrap();
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 1);

        // bit 0 not set -> nothing is sent
        let err = quantis.modules_disable(0b10).unwrap_err();
        assert!(matches!(err, Error::InvalidModuleMask(0b10)));
        let res = quantis.modules_status().unwrap();
        assert_eq!(res, 1); // still enabled: the disable command was not sent

        let err = quantis.modules_enable(0b10).unwrap_err();
        assert!(matches!(err, Error::InvalidModuleMask(0b10)));
    }

    #[test]
    fn test_clear_ais31_flag() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.clear_ais31_startup_tests_request_flag().unwrap();
        let res = quantis.ais31_startup_tests_request_flag().unwrap();
        assert_eq!(res, 0);
    }

    #[test]
    fn test_unexpected_response() {
        let fake_transport = FakeUsbTransport::default().with_control_in_limit(2);
        let mut quantis = QuantisUsb::with_fake(fake_transport);
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

    #[test]
    fn test_read_empty_buffer_sends_nothing() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.fill_bytes(&mut []).unwrap();
        assert_eq!(quantis.fake().bulk_in_requests, Vec::<usize>::new());
        assert_eq!(quantis.fake().control_in_requests, Vec::<u8>::new());
    }

    #[test]
    fn test_read_less_than_a_packet() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut buf = [0u8; 100];
        quantis.fill_bytes(&mut buf).unwrap();

        assert_eq!(quantis.fake().bulk_in_requests, [512]);
        assert_eq!(
            buf.as_slice(),
            (0..100)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_read_whole_packets() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut buf = [0u8; 1024];
        quantis.fill_bytes(&mut buf).unwrap();

        assert_eq!(quantis.fake().bulk_in_requests, [1024]);
        assert_eq!(
            buf.as_slice(),
            (0..1024)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_read_packets_and_tail() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut buf = [0u8; 1100];
        quantis.fill_bytes(&mut buf).unwrap();

        // 2 packets into `buf`, then 1 packet for the 76 bytes tail
        assert_eq!(quantis.fake().bulk_in_requests, [1024, 512]);
        assert_eq!(
            buf.as_slice(),
            (0..1100)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );

        // the other 436 bytes of the tail packet are thrown away
        let mut buf = [0u8; 512];
        quantis.fill_bytes(&mut buf).unwrap();
        assert_eq!(
            buf.as_slice(),
            (1536..2048)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_read_full_speed_packets_and_tail() {
        let transport = FakeUsbTransport::default().with_max_packet_size(64);
        let mut quantis = QuantisUsb::with_fake(transport);
        let mut buf = [0u8; 100];
        quantis.fill_bytes(&mut buf).unwrap();

        assert_eq!(quantis.fake().bulk_in_requests, [64, 64]);
        assert_eq!(
            buf.as_slice(),
            (0..100)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_read_large_buffer_is_chunked() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut buf = vec![0u8; 40_000];
        quantis.fill_bytes(&mut buf).unwrap();

        // 40 000 = 16 384 + 16 384 + 7 168 (14 packets) + 64 bytes tail
        assert_eq!(
            quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 7168, 512]
        );
        assert_eq!(
            buf,
            (0..40_000)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn test_read_short_packets() {
        let transport = FakeUsbTransport::default()
            .with_max_packet_size(64)
            .with_bulk_in_limit(10);
        let mut quantis = QuantisUsb::with_fake(transport);
        let mut buf = [0u8; 150];
        quantis.fill_bytes(&mut buf).unwrap();

        assert_eq!(
            buf.as_slice(),
            (0..150)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
        // requests are still whole packets
        let requests = &quantis.fake().bulk_in_requests;
        assert_eq!(requests.len(), 15); // 10 bytes each
        assert!(requests.iter().all(|&len| len > 0 && len % 64 == 0));
    }

    #[test]
    fn test_read_empty_transfer_is_an_error() {
        let transport = FakeUsbTransport::default().with_bulk_in_limit(0);
        let mut quantis = QuantisUsb::with_fake(transport);

        // whole packets
        let err = quantis.fill_bytes(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(err, Error::NoData));

        // tail
        let err = quantis.fill_bytes(&mut [0u8; 10]).unwrap_err();
        assert!(matches!(err, Error::NoData));
    }

    #[test]
    fn test_read_propagates_transport_errors() {
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut quantis = QuantisUsb::with_fake(transport);

        // first packets ok, tail packet fails
        let mut buf = [0xAAu8; 1100];
        let err = quantis.fill_bytes(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::Transport(TransportError::Disconnected)
        ));
        // the 1024 bytes already read are cleared
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_read_with_module_disabled() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();

        let err = quantis.fill_bytes(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(err, Error::ModuleNotReady));
        // no bulk request sent
        assert_eq!(quantis.fake().bulk_in_requests, Vec::<usize>::new());
    }

    #[test]
    fn test_read_checks_status_before_every_transfer() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.fill_bytes(&mut vec![0u8; 40_000]).unwrap();

        // 4 transfers, one status request before each
        assert_eq!(quantis.fake().bulk_in_requests.len(), 4);
        assert_eq!(
            quantis.fake().control_in_requests,
            [VendorRequest::GetModulesStatus as u8; 4]
        );
    }

    #[test]
    fn test_read_module_failure_mid_read() {
        let transport = FakeUsbTransport::default().with_module_failure_after(1);
        let mut quantis = QuantisUsb::with_fake(transport);

        let mut buf = vec![0xAAu8; 40_000];
        let err = quantis.fill_bytes(&mut buf).unwrap_err();
        assert!(matches!(err, Error::ModuleNotReady));
        // one chunk read, then the status check stops the read
        assert_eq!(quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_read_status_read_failure() {
        // status request fails -> 0 bytes instead of 1
        let transport = FakeUsbTransport::default().with_control_in_limit(0);
        let mut quantis = QuantisUsb::with_fake(transport);

        let err = quantis.fill_bytes(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedResponse {
                request: VendorRequest::GetModulesStatus,
                expected: 1,
                received: 0
            }
        ));
        assert_eq!(quantis.fake().bulk_in_requests, Vec::<usize>::new());
    }

    #[test]
    fn test_read_rejects_overreporting_transport() {
        let transport = FakeUsbTransport::default().with_bulk_in_overreport(1);
        let mut quantis = QuantisUsb::with_fake(transport);

        // whole packets
        let mut buf = [0xAAu8; 512];
        let err = quantis.fill_bytes(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::BulkOverflow {
                requested: 512,
                received: 513
            }
        ));
        assert!(buf.iter().all(|&b| b == 0));

        // tail
        let err = quantis.fill_bytes(&mut [0u8; 10]).unwrap_err();
        assert!(matches!(err, Error::BulkOverflow { .. }));
    }

    #[test]
    fn test_read_never_repeats_discarded_bytes() {
        // tail only reads -> a byte is never returned twice
        let mut quantis =
            QuantisUsb::with_fake(FakeUsbTransport::default().with_max_packet_size(64));
        let mut first = [0u8; 10];
        let mut second = [0u8; 10];
        quantis.fill_bytes(&mut first).unwrap();
        quantis.fill_bytes(&mut second).unwrap();

        assert_eq!(
            first.as_slice(),
            (0..10)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
        assert_eq!(
            second.as_slice(),
            (64..74)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_io_read_sizes() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());

        // whole packets only, the 76 bytes left are for the next read
        assert_eq!(quantis.read(&mut [0u8; 1100]).unwrap(), 1024);
        // less than a packet -> one packet
        assert_eq!(quantis.read(&mut [0u8; 100]).unwrap(), 100);
        // at most one chunk
        assert_eq!(
            quantis.read(&mut vec![0u8; 40_000]).unwrap(),
            READ_CHUNK_SIZE
        );
        // empty -> nothing sent, not EOF
        assert_eq!(quantis.read(&mut []).unwrap(), 0);

        assert_eq!(
            quantis.fake().bulk_in_requests,
            [1024, 512, READ_CHUNK_SIZE]
        );
    }

    #[test]
    fn test_io_read_full_speed() {
        let transport = FakeUsbTransport::default().with_max_packet_size(64);
        let mut quantis = QuantisUsb::with_fake(transport);

        assert_eq!(quantis.read(&mut [0u8; 100]).unwrap(), 64);
        assert_eq!(quantis.fake().bulk_in_requests, [64]);
    }

    #[test]
    fn test_io_read_exact() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut buf = [0u8; 1100];
        quantis.read_exact(&mut buf).unwrap();

        // same as `fill_bytes`
        assert_eq!(quantis.fake().bulk_in_requests, [1024, 512]);
        assert_eq!(
            buf.as_slice(),
            (0..1100)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_io_read_exact_error_clears_buffer() {
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut quantis = QuantisUsb::with_fake(transport);

        // first packets ok, tail packet fails
        let mut buf = [0xAAu8; 1100];
        let err = quantis.read_exact(&mut buf).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotConnected);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_io_read_error_keeps_the_original() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();

        let err = quantis.read(&mut [0u8; 512]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        let inner = err.get_ref().and_then(|e| e.downcast_ref::<Error>());
        assert!(matches!(inner, Some(Error::ModuleNotReady)));
    }

    #[test]
    fn test_io_read_to_end_fails() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());

        let err = quantis.read_to_end(&mut Vec::new()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        let err = quantis.read_to_string(&mut String::new()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Unsupported);
        // nothing read
        assert_eq!(quantis.fake().bulk_in_requests, Vec::<usize>::new());
    }

    #[test]
    fn test_io_copy_with_take() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());

        let mut out = Vec::new();
        let n = io::copy(&mut (&mut quantis).take(40_000), &mut out).unwrap();
        assert_eq!(n, 40_000);
        assert_eq!(out.len(), 40_000);

        // `read_to_end` works with `take`
        let mut out = Vec::new();
        (&mut quantis).take(1000).read_to_end(&mut out).unwrap();
        assert_eq!(out.len(), 1000);
    }

    #[test]
    fn test_io_small_reads_with_bufreader() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());

        let mut reader = io::BufReader::with_capacity(READ_CHUNK_SIZE, &mut quantis);
        let mut out = Vec::new();
        let mut word = [0u8; 4];
        for _ in 0..100 {
            reader.read_exact(&mut word).unwrap();
            out.extend_from_slice(&word);
        }
        drop(reader);

        // one request for 100 reads, nothing thrown away
        assert_eq!(quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        assert_eq!(
            out,
            (0..400)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
        );
    }
}
