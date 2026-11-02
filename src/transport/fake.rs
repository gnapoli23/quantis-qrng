//! Implementation of a fake Quantis device.

use crate::{
    transport::{TransportError, UsbTransport},
    usb::{MAX_BULK_PACKET_SIZE, VendorRequest},
};

#[derive(Debug)]
pub struct FakeUsbQuantis {
    state: QuantisState,
    max_packet_size: usize,
    counter: u8,
}

#[derive(Debug)]
struct QuantisState {
    status: u8,
    power: u8,
    mask: u8,
    board_version: u32,
    rate: u32,
    ais31: u8,
}

impl Default for QuantisState {
    fn default() -> Self {
        Self {
            status: 1u8, // enable
            power: 1u8,  // on
            mask: 1u8,
            board_version: 0u32, // v0
            rate: 4000u32,       // kbit/s
            ais31: 1u8,
        }
    }
}

impl Default for FakeUsbQuantis {
    fn default() -> Self {
        Self {
            state: QuantisState::default(),
            max_packet_size: MAX_BULK_PACKET_SIZE,
            counter: 0,
        }
    }
}

impl UsbTransport for FakeUsbQuantis {
    // device -> host
    // the host receives the control response for that request
    fn control_in(&mut self, request: u8, buf: &mut [u8]) -> Result<usize, TransportError> {
        let vendor_request = VendorRequest::try_from(request).map_err(|_| TransportError::Stall)?;

        let data = match vendor_request {
            VendorRequest::GetBoardVersion => {
                self.state.board_version.to_le_bytes().to_vec() // u32 -> LE
            }
            VendorRequest::GetModulesRate => self.state.rate.to_le_bytes().to_vec(),
            VendorRequest::GetModulesStatus => self.state.status.to_be_bytes().to_vec(), // u8 -> LE = BE
            VendorRequest::GetModulesPower => self.state.power.to_be_bytes().to_vec(),
            VendorRequest::GetModulesMask => self.state.mask.to_be_bytes().to_vec(),
            VendorRequest::GetAis31StartupTestsRequestFlag => {
                self.state.ais31.to_be_bytes().to_vec()
            }
            VendorRequest::ModuleEnable
            | VendorRequest::ModuleDisable
            | VendorRequest::ClearAis31StartupTestsRequestFlag => {
                return Err(TransportError::Stall);
            }
        };

        let n = buf.len().min(vendor_request.response_len());
        buf[..n].copy_from_slice(&data[..n]);

        Ok(n)
    }

    // host -> device
    // the device receives the control request and acts accordingly with its internal state
    fn control_out(&mut self, request: u8) -> Result<(), TransportError> {
        let vendor_request = VendorRequest::try_from(request).map_err(|_| TransportError::Stall)?;

        match vendor_request {
            VendorRequest::ModuleDisable => self.state.status = 0,
            VendorRequest::ModuleEnable => self.state.status = 1,
            VendorRequest::ClearAis31StartupTestsRequestFlag => self.state.ais31 = 0,
            VendorRequest::GetAis31StartupTestsRequestFlag
            | VendorRequest::GetBoardVersion
            | VendorRequest::GetModulesMask
            | VendorRequest::GetModulesPower
            | VendorRequest::GetModulesStatus
            | VendorRequest::GetModulesRate => return Err(TransportError::Stall),
        }

        Ok(())
    }

    // device -> host
    fn bulk_in(&mut self, buf: &mut [u8]) -> Result<usize, TransportError> {
        // the buffer size  must be = n*max_size
        if buf.is_empty() || buf.len() % self.max_packet_size != 0 {
            return Err(TransportError::InvalidArgument);
        }

        for b in buf.iter_mut() {
            *b = self.counter;
            self.counter = self.counter.wrapping_add(1);
        }

        Ok(buf.len())
    }

    // device -> host
    fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }
}

impl FakeUsbQuantis {
    pub fn new(max_packet_size: usize) -> Self {
        assert!(max_packet_size == 64 || max_packet_size == MAX_BULK_PACKET_SIZE);

        FakeUsbQuantis {
            state: QuantisState::default(),
            max_packet_size,
            counter: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        transport::{TransportError, UsbTransport, fake::FakeUsbQuantis},
        usb::{MAX_BULK_PACKET_SIZE, VendorRequest},
    };

    #[test]
    fn test_control_unknown_command() {
        let mut buf = [0u8; 10];
        let mut fake_quantis = FakeUsbQuantis::default();

        let res = fake_quantis.control_in(0x99, &mut buf);
        assert!(matches!(res, Err(TransportError::Stall)));
    }

    #[test]
    fn test_control_in_with_longer_buffer() {
        let mut buf = [0u8; 10];
        let mut fake_quantis = FakeUsbQuantis::default();

        let res = fake_quantis
            .control_in(VendorRequest::GetBoardVersion as u8, &mut buf)
            .unwrap();

        assert_eq!(res, 4);

        let res = fake_quantis
            .control_in(VendorRequest::GetModulesRate as u8, &mut buf)
            .unwrap();

        assert_eq!(res, 4);
    }

    #[test]
    fn test_control_in_with_shorter_buffer() {
        let mut buf = [0u8; 2];
        let mut fake_quantis = FakeUsbQuantis::default();

        let res = fake_quantis
            .control_in(VendorRequest::GetBoardVersion as u8, &mut buf)
            .unwrap();

        assert_eq!(res, 2);

        let res = fake_quantis
            .control_in(VendorRequest::GetModulesRate as u8, &mut buf)
            .unwrap();

        assert_eq!(res, 2);
    }

    #[test]
    fn test_control_module_enable_disable() {
        let mut fake_quantis = FakeUsbQuantis::default(); // by default is enabled
        let mut buf = [0u8; 4];

        // check
        let res = fake_quantis
            .control_in(VendorRequest::GetModulesStatus as u8, &mut buf)
            .unwrap();
        assert_eq!(res, 1);
        assert_eq!(buf[0], 1u8);

        // disable
        fake_quantis
            .control_out(VendorRequest::ModuleDisable as u8)
            .unwrap();

        // check
        let res = fake_quantis
            .control_in(VendorRequest::GetModulesStatus as u8, &mut buf)
            .unwrap();
        assert_eq!(res, 1);
        assert_eq!(buf[0], 0u8);

        // re-enable
        fake_quantis
            .control_out(VendorRequest::ModuleEnable as u8)
            .unwrap();

        // check
        let res = fake_quantis
            .control_in(VendorRequest::GetModulesStatus as u8, &mut buf)
            .unwrap();
        assert_eq!(res, 1);
        assert_eq!(buf[0], 1u8);
    }

    #[test]
    fn test_wrong_buffer_size_with_lowest_packet_size() {
        let mut fake_quantis = FakeUsbQuantis::new(64);

        // buf size 0
        let mut buf = [0u8; 0];
        let res = fake_quantis.bulk_in(&mut buf);
        assert!(matches!(res, Err(TransportError::InvalidArgument)));

        // buf size 32 (which is not = n * max_size) -> n must be int
        let mut buf = [0u8; 32];
        let res = fake_quantis.bulk_in(&mut buf);
        assert!(matches!(res, Err(TransportError::InvalidArgument)));

        // buf size 100
        let mut buf = [0u8; 100];
        let res = fake_quantis.bulk_in(&mut buf);
        assert!(matches!(res, Err(TransportError::InvalidArgument)));
    }

    #[test]
    fn test_good_buffer_size_with_lowest_packet_size() {
        let mut fake_quantis = FakeUsbQuantis::new(64);

        let mut buf = [0u8; 128];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 128);

        for (count, i) in buf.iter().enumerate() {
            assert_eq!(*i as usize, count);
        }
    }

    #[test]
    fn test_good_buffer_size_with_lowest_packet_size_consecutives() {
        let mut fake_quantis = FakeUsbQuantis::new(64);

        let mut buf = [0u8; 128];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 128);
        let expected: Vec<u8> = (0..128).collect();
        assert_eq!(buf.as_slice(), expected.as_slice());

        let mut buf = [0u8; 128];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 128);
        let expected: Vec<u8> = (128..=255).collect();
        assert_eq!(buf.as_slice(), expected.as_slice());
    }

    #[test]
    fn test_good_buffer_size_with_lowest_packet_size_max_read() {
        let mut fake_quantis = FakeUsbQuantis::new(64);

        let mut buf = [0u8; 512];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 512);
        let expected: Vec<u8> = (0..=255).collect();
        let expected = expected
            .into_iter()
            .chain((0..=255).collect::<Vec<u8>>())
            .collect::<Vec<u8>>();
        assert_eq!(buf.as_slice(), expected.as_slice());

        let mut buf = [0u8; 64];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 64);
        let expected: Vec<u8> = (0..64).collect();
        assert_eq!(buf.as_slice(), expected.as_slice());
    }

    #[test]
    fn test_good_buffer_size_with_max_size() {
        let mut fake_quantis = FakeUsbQuantis::new(MAX_BULK_PACKET_SIZE);

        let mut buf = [0u8; 1024];
        let res = fake_quantis.bulk_in(&mut buf).unwrap();

        assert_eq!(res, 1024);
        let expected: Vec<u8> = (0..=255).collect();
        let expected = expected
            .into_iter()
            .chain((0..=255).collect::<Vec<u8>>())
            .chain((0..=255).collect::<Vec<u8>>())
            .chain((0..=255).collect::<Vec<u8>>())
            .collect::<Vec<u8>>();
        assert_eq!(buf.as_slice(), expected.as_slice());
    }
}
