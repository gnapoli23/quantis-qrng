//! [`rand_core`] support, with the `rand_core` feature.

use std::fmt;

use rand_core::{TryRng, utils};

use crate::{
    error::Error,
    usb::{QuantisUsb, READ_CHUNK_SIZE},
};

/// Every call reads from the device, good for seeds. For many small values use
/// [`QuantisRng`].
///
/// # Example
///
/// ```no_run
/// use quantis_qrng::usb::QuantisUsb;
/// use rand::{SeedableRng, rngs::StdRng};
///
/// # fn main() -> Result<(), quantis_qrng::Error> {
/// let mut quantis = QuantisUsb::open()?;
/// let rng = StdRng::try_from_rng(&mut quantis)?;
/// # Ok(())
/// # }
/// ```
impl TryRng for QuantisUsb {
    type Error = Error;

    fn try_next_u32(&mut self) -> Result<u32, Error> {
        utils::next_word_via_fill(self)
    }

    fn try_next_u64(&mut self) -> Result<u64, Error> {
        utils::next_word_via_fill(self)
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dst)
    }
}

/// [`QuantisUsb`] with a buffer, for many small values.
///
/// One USB read fills 16 KiB, then the values come from memory. Every byte is
/// returned once and then cleared, the whole buffer is cleared on error and on
/// drop.
///
/// It doesn't implement `TryCryptoRng`, the raw output can have a small bias.
/// For keys, use it to seed a CSPRNG.
///
/// # Example
///
/// ```no_run
/// use quantis_qrng::usb::{QuantisRng, QuantisUsb};
/// use rand::{RngExt, TryRng, rand_core::UnwrapErr};
///
/// # fn main() -> Result<(), quantis_qrng::Error> {
/// let mut rng = QuantisRng::new(QuantisUsb::open()?);
///
/// let n = rng.try_next_u32()?;
/// // `UnwrapErr` for the `rand` API, it panics on device errors
/// let dice = UnwrapErr(&mut rng).random_range(1..=6);
/// # Ok(())
/// # }
/// ```
pub struct QuantisRng {
    quantis: QuantisUsb,
    buf: Buffer,
    // next byte to return, `buf.len()` -> empty
    pos: usize,
}

// cleared on drop
struct Buffer(Box<[u8]>);

impl Drop for Buffer {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

impl QuantisRng {
    /// Wraps the device, nothing is read until the first value.
    #[must_use]
    pub fn new(quantis: QuantisUsb) -> Self {
        Self {
            quantis,
            buf: Buffer(vec![0u8; READ_CHUNK_SIZE].into_boxed_slice()),
            pos: READ_CHUNK_SIZE,
        }
    }

    /// Returns the device, the buffered bytes are cleared.
    #[must_use]
    pub fn into_inner(self) -> QuantisUsb {
        self.quantis
    }

    // body of `try_fill_bytes`, on error `dst` can be partially filled
    fn fill(&mut self, dst: &mut [u8]) -> Result<(), Error> {
        let mut filled = 0;
        while filled < dst.len() {
            if self.pos == self.buf.0.len() {
                // big request -> straight into `dst`
                if dst.len() - filled >= self.buf.0.len() {
                    return self.quantis.fill_bytes(&mut dst[filled..]);
                }
                // on error `fill_bytes` clears the buffer, still empty
                self.quantis.fill_bytes(&mut self.buf.0)?;
                self.pos = 0;
            }

            let n = (self.buf.0.len() - self.pos).min(dst.len() - filled);
            let src = &mut self.buf.0[self.pos..self.pos + n];
            dst[filled..filled + n].copy_from_slice(src);
            src.fill(0); // returned only once
            self.pos += n;
            filled += n;
        }
        Ok(())
    }
}

impl TryRng for QuantisRng {
    type Error = Error;

    fn try_next_u32(&mut self) -> Result<u32, Error> {
        utils::next_word_via_fill(self)
    }

    fn try_next_u64(&mut self) -> Result<u64, Error> {
        utils::next_word_via_fill(self)
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Error> {
        let result = self.fill(dst);
        if result.is_err() {
            // no partial data
            dst.fill(0);
        }
        result
    }
}

// no buffered bytes in the output
impl fmt::Debug for QuantisRng {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuantisRng")
            .field("quantis", &self.quantis)
            .field("buffered", &(self.buf.0.len() - self.pos))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, TryRng, rngs::StdRng};

    use crate::{
        error::Error,
        usb::{
            QuantisRng, QuantisUsb, READ_CHUNK_SIZE,
            transport::{TransportError, fake::FakeUsbTransport},
        },
    };

    // the fake sends a counter -> byte `k` is `k % 256`
    fn stream(range: std::ops::Range<usize>) -> Vec<u8> {
        range.map(|k| (k % 256).to_le_bytes()[0]).collect()
    }

    fn rng(transport: FakeUsbTransport) -> QuantisRng {
        QuantisRng::new(QuantisUsb::with_fake(transport))
    }

    #[test]
    fn test_usb_try_rng() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());

        // little-endian, one packet each (512 % 256 = 0, the counter starts again)
        assert_eq!(quantis.try_next_u32().unwrap(), 0x0302_0100);
        assert_eq!(quantis.try_next_u64().unwrap(), 0x0706_0504_0302_0100);
        assert_eq!(quantis.fake().bulk_in_requests, [512, 512]);
    }

    #[test]
    fn test_usb_seed() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        StdRng::try_from_rng(&mut quantis).unwrap();
        // 32 bytes -> one packet
        assert_eq!(quantis.fake().bulk_in_requests, [512]);
    }

    #[test]
    fn test_rng_values_come_from_the_buffer() {
        let mut rng = rng(FakeUsbTransport::default());
        // nothing read before the first value
        assert_eq!(rng.quantis.fake().bulk_in_requests, Vec::<usize>::new());

        let mut out = Vec::new();
        for _ in 0..100 {
            out.extend_from_slice(&rng.try_next_u32().unwrap().to_le_bytes());
        }
        out.extend_from_slice(&rng.try_next_u64().unwrap().to_le_bytes());

        // one read for all, in order, nothing skipped
        assert_eq!(rng.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        assert_eq!(out, stream(0..408));
    }

    #[test]
    fn test_rng_bytes_cleared_after_use() {
        let mut rng = rng(FakeUsbTransport::default());
        let mut buf = [0u8; 100];
        rng.try_fill_bytes(&mut buf).unwrap();

        assert_eq!(buf.as_slice(), stream(0..100).as_slice());
        assert!(rng.buf.0[..100].iter().all(|&b| b == 0));
        assert_eq!(&rng.buf.0[100..200], stream(100..200).as_slice());
    }

    #[test]
    fn test_rng_refill() {
        let mut rng = rng(FakeUsbTransport::default());
        let mut buf = vec![0u8; READ_CHUNK_SIZE - 10];
        rng.try_fill_bytes(&mut buf).unwrap();

        // 10 bytes from the buffer, then a new read
        let mut buf = [0u8; 20];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(
            buf.as_slice(),
            stream(READ_CHUNK_SIZE - 10..READ_CHUNK_SIZE + 10).as_slice()
        );
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE]
        );
    }

    #[test]
    fn test_rng_big_request_skips_the_buffer() {
        let mut rng = rng(FakeUsbTransport::default());
        let mut buf = vec![0u8; 40_000];
        rng.try_fill_bytes(&mut buf).unwrap();

        // straight into `buf`, the buffer stays empty
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 7168, 512]
        );
        assert_eq!(buf, stream(0..40_000));
        assert_eq!(rng.pos, READ_CHUNK_SIZE);
    }

    #[test]
    fn test_rng_buffered_bytes_first() {
        let mut rng = rng(FakeUsbTransport::default());
        rng.try_next_u32().unwrap();

        // the rest of the buffer, then the big request straight into `buf`
        let mut buf = vec![0u8; 2 * READ_CHUNK_SIZE];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(
            &buf[..READ_CHUNK_SIZE - 4],
            stream(4..READ_CHUNK_SIZE).as_slice()
        );
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 512]
        );
    }

    #[test]
    fn test_rng_error_clears_everything() {
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut rng = rng(transport);
        rng.try_next_u32().unwrap();

        // the rest of the buffer, then the read fails
        let mut buf = vec![0xAAu8; READ_CHUNK_SIZE];
        let err = rng.try_fill_bytes(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::Transport(TransportError::Disconnected)
        ));
        assert!(buf.iter().all(|&b| b == 0));
        assert!(rng.buf.0.iter().all(|&b| b == 0));
        assert_eq!(rng.pos, READ_CHUNK_SIZE);
    }

    #[test]
    fn test_rng_module_disabled() {
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();
        let mut rng = QuantisRng::new(quantis);

        assert!(matches!(rng.try_next_u32(), Err(Error::ModuleNotReady)));
        // nothing buffered
        assert_eq!(rng.pos, READ_CHUNK_SIZE);
    }

    #[test]
    fn test_rng_debug_hides_the_data() {
        let mut rng = rng(FakeUsbTransport::default());
        rng.try_next_u32().unwrap();
        let debug = format!("{rng:?}");
        assert!(debug.contains(&format!("buffered: {}", READ_CHUNK_SIZE - 4)));
    }
}
