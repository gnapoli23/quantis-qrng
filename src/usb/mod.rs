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

    /// Fills `buf` with random bytes read from the device.
    ///
    /// Returns `Ok(())` only when **every** byte of `buf` has been written
    /// with data received from the device in this call. An empty buffer
    /// returns immediately without talking to the device.
    ///
    /// On error `buf` is overwritten with zeros, so that a caller that
    /// ignores the error cannot mistake a partially filled buffer for random
    /// data. The zeros are not random: never use `buf` after an error.
    ///
    /// # How the device is read
    ///
    /// * Before every bulk transfer the module status is read, like the
    ///   original library does; a status of `0` stops the read with
    ///   [`Error::ModuleNotReady`]. The original library checks it every 512
    ///   bytes, this method every transfer (at most [`READ_CHUNK_SIZE`]
    ///   bytes).
    /// * The device only accepts bulk requests for whole packets of
    ///   [`max_packet_size`](Self::max_packet_size) bytes. The largest
    ///   multiple of the packet size is read straight into `buf`; the tail
    ///   shorter than a packet is read as one more packet into an internal
    ///   buffer, and the bytes that do not fit in `buf` are discarded, never
    ///   returned by a later call.
    /// * A short packet is not an error: the read goes on until `buf` is
    ///   full. Every transfer either fails or adds at least one byte, so the
    ///   method always terminates.
    ///
    /// # Errors
    ///
    /// * [`Error::ModuleNotReady`] if the module status is `0` before a
    ///   transfer.
    /// * [`Error::NoData`] if a transfer completes without any data.
    /// * [`Error::BulkOverflow`] if the transport reports more bytes than
    ///   requested (a bug in the transport).
    /// * [`Error::UnexpectedDescriptor`] if the bulk endpoint reports a
    ///   maximum packet size of 0.
    /// * [`Error::Transport`] or [`Error::UnexpectedResponse`] if a transfer
    ///   fails, for example on timeout or disconnection.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        let result = self.fill(buf);
        if result.is_err() {
            // do not leave partial device output behind
            buf.fill(0);
        }
        result
    }

    // The body of `read`: on error `buf` may be partially filled.
    fn fill(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        if buf.is_empty() {
            return Ok(());
        }

        let packet_size = self.max_packet_size();
        if packet_size == 0 {
            return Err(Error::UnexpectedDescriptor);
        }
        // largest request: a whole number of packets, at least one
        let max_chunk = (READ_CHUNK_SIZE / packet_size).max(1) * packet_size;

        // one packet, for the tail shorter than a packet; allocated only if needed
        let mut scratch: Vec<u8> = Vec::new();
        let mut filled = 0;

        while filled < buf.len() {
            self.ensure_module_ready()?;

            let remaining = buf.len() - filled;
            let added = if remaining >= packet_size {
                // whole packets, straight into `buf`
                let chunk = (remaining - remaining % packet_size).min(max_chunk);
                let n = self.transport.bulk_in(&mut buf[filled..filled + chunk])?;
                check_bulk_len(n, chunk)?;
                n
            } else {
                // tail: one packet into `scratch`, keep only what fits
                if scratch.is_empty() {
                    scratch = vec![0u8; packet_size];
                }
                let n = self.transport.bulk_in(&mut scratch)?;
                check_bulk_len(n, packet_size)?;
                let take = n.min(remaining);
                buf[filled..filled + take].copy_from_slice(&scratch[..take]);
                take
            };

            // `added >= 1` (checked above): the loop always makes progress.
            // After a short packet `filled` is no longer a multiple of the
            // packet size; that is fine, only request lengths must be.
            filled += added;
        }

        // best effort: do not leave the discarded tail bytes in freed memory
        scratch.fill(0);
        std::hint::black_box(&scratch);

        Ok(())
    }

    // Fails with `Error::ModuleNotReady` if the module status is 0.
    fn ensure_module_ready(&mut self) -> Result<(), Error> {
        match self.modules_status()? {
            0 => Err(Error::ModuleNotReady),
            _ => Ok(()),
        }
    }
}

/// Checks the length `received` of a successful bulk transfer of `requested`
/// bytes.
///
/// # Bug of the original library
///
/// In the original C library (`QuantisUsbRead` in `QuantisUsb_Unix.c`), a
/// bulk transfer that succeeds with fewer bytes than a packet ends the read
/// with:
///
/// ```c
/// if ((result < 0) || (transferred != (int)_privateData->usbMaxPacketSize))
/// {
///   return result;
/// }
/// ```
///
/// After a successful transfer `result` is `LIBUSB_SUCCESS` (`0`), so the
/// read "succeeds" with 0 bytes and stops, leaving the rest of the caller's
/// buffer unfilled. The C++ wrapper (`idQ::Quantis::Read`, `Quantis_Cpp.cpp`)
/// only treats negative values as errors and returns its buffer, a
/// `std::string` resized to the requested size, so the missing part is made
/// of zero bytes; `EasyQuantis` then writes it to the output file as random
/// data.
///
/// Here a short transfer is never a success by itself: [`QuantisUsb::read`]
/// keeps reading until the buffer is full, an empty transfer is an error
/// ([`Error::NoData`]), and so is a length larger than requested.
fn check_bulk_len(received: usize, requested: usize) -> Result<(), Error> {
    if received == 0 {
        Err(Error::NoData)
    } else if received > requested {
        Err(Error::BulkOverflow {
            requested,
            received,
        })
    } else {
        Ok(())
    }
}

/// Largest bulk request sent by [`QuantisUsb::read`], in bytes.
///
/// A Quantis USB 4M streams about 500 000 bytes/s and every transfer has a
/// timeout of [`TRANSFER_TIMEOUT`](protocol::TRANSFER_TIMEOUT) (1 s): a chunk
/// must be well below what the device produces in that time. 16 KiB take
/// about 33 ms and are a multiple of both packet sizes (64 and 512).
pub const READ_CHUNK_SIZE: usize = 16 * 1024;

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
        usb::{
            QuantisUsb, READ_CHUNK_SIZE,
            protocol::VendorRequest,
            transport::{TransportError, fake::FakeUsbTransport},
        },
    };

    // The fake streams a counter: byte `k` of the stream is `k % 256`.
    fn stream(range: std::ops::Range<usize>) -> Vec<u8> {
        range.map(|k| (k % 256).to_le_bytes()[0]).collect()
    }

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

    #[test]
    fn test_read_empty_buffer_sends_nothing() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        quantis.read(&mut []).unwrap();
        assert!(quantis.transport.bulk_in_requests.is_empty());
        assert!(quantis.transport.control_in_requests.is_empty());
    }

    #[test]
    fn test_read_less_than_a_packet() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        let mut buf = [0u8; 100];
        quantis.read(&mut buf).unwrap();

        assert_eq!(quantis.transport.bulk_in_requests, [512]);
        assert_eq!(buf.as_slice(), stream(0..100).as_slice());
    }

    #[test]
    fn test_read_whole_packets() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        let mut buf = [0u8; 1024];
        quantis.read(&mut buf).unwrap();

        assert_eq!(quantis.transport.bulk_in_requests, [1024]);
        assert_eq!(buf.as_slice(), stream(0..1024).as_slice());
    }

    #[test]
    fn test_read_packets_and_tail() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        let mut buf = [0u8; 1100];
        quantis.read(&mut buf).unwrap();

        // 2 packets straight into `buf`, then one more packet for the 76-byte tail
        assert_eq!(quantis.transport.bulk_in_requests, [1024, 512]);
        assert_eq!(buf.as_slice(), stream(0..1100).as_slice());

        // the 436 bytes of the tail packet that did not fit are discarded
        let mut buf = [0u8; 512];
        quantis.read(&mut buf).unwrap();
        assert_eq!(buf.as_slice(), stream(1536..2048).as_slice());
    }

    #[test]
    fn test_read_full_speed_packets_and_tail() {
        let transport = FakeUsbTransport::default().with_max_packet_size(64);
        let mut quantis = QuantisUsb::new(transport);
        let mut buf = [0u8; 100];
        quantis.read(&mut buf).unwrap();

        assert_eq!(quantis.transport.bulk_in_requests, [64, 64]);
        assert_eq!(buf.as_slice(), stream(0..100).as_slice());
    }

    #[test]
    fn test_read_large_buffer_is_chunked() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        let mut buf = vec![0u8; 40_000];
        quantis.read(&mut buf).unwrap();

        // 40 000 = 16 384 + 16 384 + 7 168 (14 packets) + 64-byte tail
        assert_eq!(
            quantis.transport.bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 7168, 512]
        );
        assert_eq!(buf, stream(0..40_000));
    }

    #[test]
    fn test_read_short_packets() {
        let transport = FakeUsbTransport::default()
            .with_max_packet_size(64)
            .with_bulk_in_limit(10);
        let mut quantis = QuantisUsb::new(transport);
        let mut buf = [0u8; 150];
        quantis.read(&mut buf).unwrap();

        assert_eq!(buf.as_slice(), stream(0..150).as_slice());
        // every request is still a whole number of packets
        let requests = &quantis.transport.bulk_in_requests;
        assert_eq!(requests.len(), 15); // 10 bytes each
        assert!(requests.iter().all(|&len| len > 0 && len % 64 == 0));
    }

    #[test]
    fn test_read_empty_transfer_is_an_error() {
        let transport = FakeUsbTransport::default().with_bulk_in_limit(0);
        let mut quantis = QuantisUsb::new(transport);

        // whole packets
        let err = quantis.read(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(err, Error::NoData));

        // tail
        let err = quantis.read(&mut [0u8; 10]).unwrap_err();
        assert!(matches!(err, Error::NoData));
    }

    #[test]
    fn test_read_propagates_transport_errors() {
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut quantis = QuantisUsb::new(transport);

        // the first packets arrive, the tail packet fails
        let mut buf = [0xAAu8; 1100];
        let err = quantis.read(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::Transport(TransportError::Disconnected)
        ));
        // the 1024 bytes already received are wiped
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_read_with_module_disabled() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();

        let err = quantis.read(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(err, Error::ModuleNotReady));
        // stopped before asking for data
        assert!(quantis.transport.bulk_in_requests.is_empty());
    }

    #[test]
    fn test_read_checks_status_before_every_transfer() {
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default());
        quantis.read(&mut vec![0u8; 40_000]).unwrap();

        // 4 transfers, each preceded by a status request and nothing else
        assert_eq!(quantis.transport.bulk_in_requests.len(), 4);
        assert_eq!(
            quantis.transport.control_in_requests,
            [VendorRequest::GetModulesStatus as u8; 4]
        );
    }

    #[test]
    fn test_read_module_failure_mid_read() {
        let transport = FakeUsbTransport::default().with_module_failure_after(1);
        let mut quantis = QuantisUsb::new(transport);

        let mut buf = vec![0xAAu8; 40_000];
        let err = quantis.read(&mut buf).unwrap_err();
        assert!(matches!(err, Error::ModuleNotReady));
        // one chunk arrived, then the status check stopped the read
        assert_eq!(quantis.transport.bulk_in_requests, [READ_CHUNK_SIZE]);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_read_status_read_failure() {
        // the status request itself fails: 0 bytes instead of 1
        let transport = FakeUsbTransport::default().with_control_in_limit(0);
        let mut quantis = QuantisUsb::new(transport);

        let err = quantis.read(&mut [0u8; 512]).unwrap_err();
        assert!(matches!(
            err,
            Error::UnexpectedResponse {
                request: VendorRequest::GetModulesStatus,
                expected: 1,
                received: 0
            }
        ));
        assert!(quantis.transport.bulk_in_requests.is_empty());
    }

    #[test]
    fn test_read_rejects_overreporting_transport() {
        let transport = FakeUsbTransport::default().with_bulk_in_overreport(1);
        let mut quantis = QuantisUsb::new(transport);

        // whole packets
        let mut buf = [0xAAu8; 512];
        let err = quantis.read(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::BulkOverflow {
                requested: 512,
                received: 513
            }
        ));
        assert!(buf.iter().all(|&b| b == 0));

        // tail
        let err = quantis.read(&mut [0u8; 10]).unwrap_err();
        assert!(matches!(err, Error::BulkOverflow { .. }));
    }

    #[test]
    fn test_read_never_repeats_discarded_bytes() {
        // many tail-only reads: every byte of the stream is returned at most once
        let mut quantis = QuantisUsb::new(FakeUsbTransport::default().with_max_packet_size(64));
        let mut first = [0u8; 10];
        let mut second = [0u8; 10];
        quantis.read(&mut first).unwrap();
        quantis.read(&mut second).unwrap();

        assert_eq!(first.as_slice(), stream(0..10).as_slice());
        assert_eq!(second.as_slice(), stream(64..74).as_slice());
    }
}
