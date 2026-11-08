//! [`rand_core`] support, with the `rand_core` feature.

use std::fmt;

use rand_core::{TryCryptoRng, TryRng, utils};

use crate::{
    error::Error,
    extractor::Matrix,
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
/// For keys use [`QuantisCryptoRng`], or seed a CSPRNG.
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
    matrix: Option<Matrix>, // if present, buffer filled with extracted data
    raw: Buffer,            // data before the extraction, empty without matrix
    buf: Buffer,            // output
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
            matrix: None,              // default
            raw: Buffer(Box::new([])), // empty
            buf: Buffer(vec![0u8; READ_CHUNK_SIZE].into_boxed_slice()),
            pos: READ_CHUNK_SIZE,
        }
    }

    /// Returns the device, the buffered bytes are cleared.
    #[must_use]
    pub fn into_inner(self) -> QuantisUsb {
        self.quantis
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
        let mut filled = 0;
        while filled < dst.len() {
            if self.pos == self.buf.0.len() {
                // with the matrix always through the buffer, `dst` is not made of
                // whole blocks
                if let Some(matrix) = &self.matrix {
                    if let Err(err) = self.quantis.fill_bytes(&mut self.raw.0) {
                        // no partial data (`fill_bytes` already cleared `raw`)
                        dst.fill(0);
                        return Err(err);
                    }
                    matrix.extract(&self.raw.0, &mut self.buf.0);
                    // raw data not needed anymore
                    self.raw.0.fill(0);
                } else {
                    // big request -> straight into `dst`, otherwise into the buffer
                    let big = dst.len() - filled >= self.buf.0.len();
                    let target = if big {
                        &mut dst[filled..]
                    } else {
                        &mut self.buf.0[..]
                    };
                    if let Err(err) = self.quantis.fill_bytes(target) {
                        // no partial data (`fill_bytes` already cleared `target`)
                        dst.fill(0);
                        return Err(err);
                    }
                    if big {
                        return Ok(());
                    }
                }
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

// no buffered bytes in the output
impl fmt::Debug for QuantisRng {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuantisRng")
            .field("quantis", &self.quantis)
            .field("buffered", &(self.buf.0.len() - self.pos))
            .field("matrix", &self.matrix)
            .finish_non_exhaustive()
    }
}

/// [`QuantisRng`] with randomness extraction, for keys.
///
/// The buffer is filled with data extracted by the [`Matrix`]: one USB read of
/// 16 KiB -> 12 KiB with 1024x768. Without the bias, so it implements
/// `TryCryptoRng` and [`UnwrapErr`](rand_core::UnwrapErr) gives a `CryptoRng`.
///
/// # Example
///
/// A Bitcoin seed (BIP39, 24 words) with the [`bip39`](https://docs.rs/bip39)
/// crate, with a XOR of the OS entropy: the seed is good if at least one of
/// the two is.
///
/// ```no_run
/// use quantis_qrng::{
///     extractor::Matrix,
///     usb::{QuantisCryptoRng, QuantisUsb},
/// };
/// use rand::{TryRng, rngs::SysRng};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // see `contrib/extractor` in the repository
/// let matrix = Matrix::new(&std::fs::read("matrix.dat")?, 1024, 768)?;
/// let mut rng = QuantisCryptoRng::new(QuantisUsb::open()?, matrix);
///
/// // 256 bits -> 24 words
/// let mut entropy = [0u8; 32];
/// rng.try_fill_bytes(&mut entropy)?;
///
/// // XOR with the OS entropy
/// let mut os = [0u8; 32];
/// SysRng.try_fill_bytes(&mut os)?;
/// for (e, o) in entropy.iter_mut().zip(&os) {
///     *e ^= o;
/// }
/// os.fill(0);
///
/// let mnemonic = bip39::Mnemonic::from_entropy(&entropy)?;
/// entropy.fill(0);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct QuantisCryptoRng(QuantisRng);

impl QuantisCryptoRng {
    /// Wraps a device and the specified extractor matrix.
    #[must_use]
    pub fn new(quantis: QuantisUsb, matrix: Matrix) -> Self {
        // whole blocks, 16 KiB of raw data -> 12 KiB with 1024x768
        let (bytes_in, bytes_out) = (matrix.size_in / 8, matrix.size_out / 8);
        let blocks = (READ_CHUNK_SIZE / bytes_in).max(1);

        Self(QuantisRng {
            quantis,
            matrix: Some(matrix),
            raw: Buffer(vec![0u8; blocks * bytes_in].into_boxed_slice()),
            buf: Buffer(vec![0u8; blocks * bytes_out].into_boxed_slice()),
            pos: blocks * bytes_out,
        })
    }

    /// Returns the device, the buffered bytes are cleared.
    #[must_use]
    pub fn into_inner(self) -> QuantisUsb {
        self.0.into_inner()
    }
}

impl TryRng for QuantisCryptoRng {
    type Error = Error;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        self.0.try_next_u32()
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        self.0.try_next_u64()
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        self.0.try_fill_bytes(dst)
    }
}

impl TryCryptoRng for QuantisCryptoRng {}

#[cfg(test)]
mod tests {
    use rand::{
        CryptoRng, SeedableRng, TryRng,
        rand_core::UnwrapErr,
        rngs::{StdRng, SysRng},
    };

    use crate::{
        error::Error,
        extractor::Matrix,
        usb::{
            QuantisCryptoRng, QuantisRng, QuantisUsb, READ_CHUNK_SIZE,
            transport::{TransportError, fake::FakeUsbTransport},
        },
    };

    // the fake sends a counter -> byte `k` is `k % 256`. the test matrices have
    // any content, the output is compared with `Matrix::extract`

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
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        // nothing read before the first value
        assert_eq!(rng.quantis.fake().bulk_in_requests, Vec::<usize>::new());

        let mut out = Vec::new();
        for _ in 0..100 {
            out.extend_from_slice(&rng.try_next_u32().unwrap().to_le_bytes());
        }
        out.extend_from_slice(&rng.try_next_u64().unwrap().to_le_bytes());

        // one read for all, in order, nothing skipped
        assert_eq!(rng.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        assert_eq!(
            out,
            (0..408)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
        );
    }

    #[test]
    fn test_rng_bytes_cleared_after_use() {
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        let mut buf = [0u8; 100];
        rng.try_fill_bytes(&mut buf).unwrap();

        assert_eq!(
            buf.as_slice(),
            (0..100)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
        assert!(rng.buf.0[..100].iter().all(|&b| b == 0));
        assert_eq!(
            &rng.buf.0[100..200],
            (100..200)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
    }

    #[test]
    fn test_rng_refill() {
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        let mut buf = vec![0u8; READ_CHUNK_SIZE - 10];
        rng.try_fill_bytes(&mut buf).unwrap();

        // 10 bytes from the buffer, then a new read
        let mut buf = [0u8; 20];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(
            buf.as_slice(),
            (READ_CHUNK_SIZE - 10..READ_CHUNK_SIZE + 10)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE]
        );
    }

    #[test]
    fn test_rng_big_request_skips_the_buffer() {
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        let mut buf = vec![0u8; 40_000];
        rng.try_fill_bytes(&mut buf).unwrap();

        // straight into `buf`, the buffer stays empty
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 7168, 512]
        );
        assert_eq!(
            buf,
            (0..40_000)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
        );
        assert_eq!(rng.pos, READ_CHUNK_SIZE);
    }

    #[test]
    fn test_rng_buffered_bytes_first() {
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        rng.try_next_u32().unwrap();

        // the rest of the buffer, then the big request straight into `buf`
        let mut buf = vec![0u8; 2 * READ_CHUNK_SIZE];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(
            &buf[..READ_CHUNK_SIZE - 4],
            (4..READ_CHUNK_SIZE)
                .map(|k: usize| (k % 256).to_le_bytes()[0])
                .collect::<Vec<u8>>()
                .as_slice()
        );
        assert_eq!(
            rng.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE, 512]
        );
    }

    #[test]
    fn test_rng_error_clears_everything() {
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(transport));
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
        let mut rng = QuantisRng::new(QuantisUsb::with_fake(FakeUsbTransport::default()));
        rng.try_next_u32().unwrap();
        let debug = format!("{rng:?}");
        assert!(debug.contains(&format!("buffered: {}", READ_CHUNK_SIZE - 4)));
    }

    #[test]
    fn test_crypto_rng_output_is_extracted() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        // nothing read before the first value
        assert_eq!(rng.0.quantis.fake().bulk_in_requests, Vec::<usize>::new());

        // 16 KiB -> 12 KiB
        let raw: Vec<u8> = (0..READ_CHUNK_SIZE)
            .map(|k: usize| (k % 256).to_le_bytes()[0])
            .collect();
        let mut expected = vec![0u8; 12 * 1024];
        Matrix::new(&matrix, 1024, 768)
            .unwrap()
            .extract(&raw, &mut expected);
        let mut buf = vec![0u8; 12 * 1024];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(buf, expected);
        assert_eq!(rng.0.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        // raw data cleared after the extraction
        assert!(rng.0.raw.0.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_crypto_rng_refill() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        let mut buf = vec![0u8; 12 * 1024];
        rng.try_fill_bytes(&mut buf).unwrap();

        // buffer empty -> a new read, same data (16384 % 256 = 0)
        let raw: Vec<u8> = (0..READ_CHUNK_SIZE)
            .map(|k: usize| (k % 256).to_le_bytes()[0])
            .collect();
        let mut expected = vec![0u8; 12 * 1024];
        Matrix::new(&matrix, 1024, 768)
            .unwrap()
            .extract(&raw, &mut expected);
        let n = rng.try_next_u32().unwrap();
        assert_eq!(n.to_le_bytes(), expected[..4]);
        assert_eq!(
            rng.0.quantis.fake().bulk_in_requests,
            [READ_CHUNK_SIZE, READ_CHUNK_SIZE]
        );
    }

    #[test]
    fn test_crypto_rng_big_request_through_the_buffer() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        let mut buf = vec![0u8; 40_000];
        rng.try_fill_bytes(&mut buf).unwrap();

        // 40000 / 12288 -> 4 reads of the same data (16384 % 256 = 0), nothing
        // straight into `buf`
        let raw: Vec<u8> = (0..READ_CHUNK_SIZE)
            .map(|k: usize| (k % 256).to_le_bytes()[0])
            .collect();
        let mut expected = vec![0u8; 12 * 1024];
        Matrix::new(&matrix, 1024, 768)
            .unwrap()
            .extract(&raw, &mut expected);
        assert_eq!(buf, expected.repeat(4)[..40_000]);
        assert_eq!(rng.0.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE; 4]);
    }

    #[test]
    fn test_crypto_rng_other_sizes() {
        // 2048x1792 -> 64 blocks of 256 bytes, 16 KiB -> 14 KiB
        let matrix: Vec<u8> = (0..2048 * 1792 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 2048, 1792).unwrap());
        assert_eq!(rng.0.raw.0.len(), READ_CHUNK_SIZE);
        assert_eq!(rng.0.buf.0.len(), 14 * 1024);

        let raw: Vec<u8> = (0..READ_CHUNK_SIZE)
            .map(|k: usize| (k % 256).to_le_bytes()[0])
            .collect();
        let mut expected = vec![0u8; 14 * 1024];
        Matrix::new(&matrix, 2048, 1792)
            .unwrap()
            .extract(&raw, &mut expected);
        let mut buf = vec![0u8; 14 * 1024];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert_eq!(buf, expected);
        assert_eq!(rng.0.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
    }

    #[test]
    fn test_crypto_rng_error_clears_everything() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let transport = FakeUsbTransport::default().with_bulk_in_disconnect_after(1);
        let mut rng = QuantisCryptoRng::new(
            QuantisUsb::with_fake(transport),
            Matrix::new(&matrix, 1024, 768).unwrap(),
        );
        rng.try_next_u32().unwrap();

        // the rest of the buffer, then the read fails
        let mut buf = vec![0xAAu8; 12 * 1024];
        let err = rng.try_fill_bytes(&mut buf).unwrap_err();
        assert!(matches!(
            err,
            Error::Transport(TransportError::Disconnected)
        ));
        assert!(buf.iter().all(|&b| b == 0));
        assert!(rng.0.raw.0.iter().all(|&b| b == 0));
        assert!(rng.0.buf.0.iter().all(|&b| b == 0));
        assert_eq!(rng.0.pos, rng.0.buf.0.len());
    }

    #[test]
    fn test_crypto_rng_module_disabled() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let mut quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        quantis.modules_disable(1).unwrap();
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());

        assert!(matches!(rng.try_next_u32(), Err(Error::ModuleNotReady)));
        assert_eq!(rng.0.pos, rng.0.buf.0.len());
    }

    #[test]
    fn test_crypto_rng_is_a_crypto_rng() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        // it doesn't compile without `TryCryptoRng`
        let _: &mut dyn CryptoRng = &mut UnwrapErr(&mut rng);

        // seed, 32 bytes from the buffer
        StdRng::try_from_rng(&mut rng).unwrap();
        assert_eq!(rng.0.quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
        assert_eq!(rng.0.pos, 32);
    }

    #[test]
    fn test_crypto_rng_into_inner() {
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        rng.try_next_u32().unwrap();

        let quantis = rng.into_inner();
        assert_eq!(quantis.fake().bulk_in_requests, [READ_CHUNK_SIZE]);
    }

    #[test]
    fn test_crypto_rng_bip39() {
        // same steps of the example
        let matrix: Vec<u8> = (0..1024 * 768 / 8)
            .map(|i: usize| (i * 167 % 251).to_le_bytes()[0])
            .collect();
        let quantis = QuantisUsb::with_fake(FakeUsbTransport::default());
        let mut rng = QuantisCryptoRng::new(quantis, Matrix::new(&matrix, 1024, 768).unwrap());
        let mut entropy = [0u8; 32];
        rng.try_fill_bytes(&mut entropy).unwrap();

        let raw: Vec<u8> = (0..READ_CHUNK_SIZE)
            .map(|k: usize| (k % 256).to_le_bytes()[0])
            .collect();
        let mut expected = vec![0u8; 12 * 1024];
        Matrix::new(&matrix, 1024, 768)
            .unwrap()
            .extract(&raw, &mut expected);
        assert_eq!(entropy, expected[..32]);

        let mut os = [0u8; 32];
        SysRng.try_fill_bytes(&mut os).unwrap();
        for (e, o) in entropy.iter_mut().zip(&os) {
            *e ^= o;
        }
        // XOR twice -> the Quantis entropy again
        let xored = entropy;
        for (e, o) in entropy.iter_mut().zip(&os) {
            *e ^= o;
        }
        assert_eq!(entropy, expected[..32]);

        let mnemonic = bip39::Mnemonic::from_entropy(&xored).unwrap();
        assert_eq!(mnemonic.word_count(), 24);
        assert_eq!(mnemonic.to_entropy(), xored);
    }
}
