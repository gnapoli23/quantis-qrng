//! Randomness extractor of `QuantisExtensions`, removes the bias of the raw
//! output.
//!
//! A matrix * vector multiplication on bits: every block of `n` input bits
//! gives `k` output bits, with `n > k` (1024 and 768 in `EasyQuantis`).
//!
//! Both `n` and `k` must be multiples of 64.

use std::fmt;

use crate::Error;

/// Extractor matrix, `k` rows of `n` bits.
pub struct Matrix {
    pub(crate) size_in: usize,  // n -> bits
    pub(crate) size_out: usize, // k -> bits
    rows: Box<[u64]>,           // rows made of n/64 words
}

impl Matrix {
    /// Creates a new extractor matrix.
    ///
    /// # Errors
    /// [`Error::InvalidMatrix`] if the sizes are not multiples of 64, `size_in`
    /// is not greater than `size_out` or `matrix` is too short.
    pub fn new(matrix: &[u8], size_in: usize, size_out: usize) -> Result<Self, Error> {
        // check size_in and size_out
        if size_in % 64 != 0 {
            return Err(Error::InvalidMatrix(
                "`size_in` is not a multiple of 64".into(),
            ));
        }

        // 0 -> no output, the rng would never end
        if size_out == 0 || size_out % 64 != 0 {
            return Err(Error::InvalidMatrix(
                "`size_out` is not a positive multiple of 64".into(),
            ));
        }

        if size_in <= size_out {
            return Err(Error::InvalidMatrix(
                "`size_in` must be greater than `size_out`".into(),
            ));
        }

        // k rows of n bits, the rest is not used (the IDQ file is bigger)
        let len = size_in
            .checked_mul(size_out)
            .map_or(usize::MAX, |bits| bits / 8);
        let Some(matrix) = matrix.get(..len) else {
            return Err(Error::InvalidMatrix(format!(
                "{} bytes, {len} needed for {size_in}x{size_out}",
                matrix.len()
            )));
        };

        // little-endian, like the C library on x86
        let rows = matrix
            .chunks_exact(8)
            .map(|word| u64::from_le_bytes(std::array::from_fn(|i| word[i])))
            .collect();

        Ok(Self {
            size_in,
            size_out,
            rows,
        })
    }

    /// Extracts `input` into `output`, block by block: `size_in / 8` bytes of
    /// `input` -> `size_out / 8` bytes of `output`.
    ///
    /// # Panics
    ///
    /// If `input` and `output` are not the same number of whole blocks.
    pub fn extract(&self, input: &[u8], output: &mut [u8]) {
        let (bytes_in, bytes_out) = (self.size_in / 8, self.size_out / 8);
        assert!(
            input.len() % bytes_in == 0
                && output.len() % bytes_out == 0
                && input.len() / bytes_in == output.len() / bytes_out,
            "`input` and `output` must be the same number of whole blocks"
        );

        let words_in = self.size_in / 64;
        let mut block = vec![0u64; words_in];
        for (input, output) in input
            .chunks_exact(bytes_in)
            .zip(output.chunks_exact_mut(bytes_out))
        {
            // little-endian, like the matrix
            for (word, bytes) in block.iter_mut().zip(input.chunks_exact(8)) {
                *word = u64::from_le_bytes(std::array::from_fn(|i| bytes[i]));
            }

            // 64 rows -> one output word, row `r` -> bit `r % 64`
            for (rows, output) in self
                .rows
                .chunks_exact(64 * words_in)
                .zip(output.chunks_exact_mut(8))
            {
                let mut word = 0u64;
                for (bit, row) in rows.chunks_exact(words_in).enumerate() {
                    // parity of `row & block`
                    let and = row.iter().zip(&block).fold(0, |acc, (r, b)| acc ^ (r & b));
                    word |= u64::from(and.count_ones() & 1) << bit;
                }
                output.copy_from_slice(&word.to_le_bytes());
            }
        }

        // raw input, cleared like the buffer of `QuantisRng`
        block.fill(0);
        std::hint::black_box(&block);
    }
}

// the rows are not printed
impl fmt::Debug for Matrix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matrix")
            .field("size_in", &self.size_in)
            .field("size_out", &self.size_out)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use crate::{Error, extractor::Matrix};

    #[test]
    fn test_same_output_of_the_c_library() {
        // only in the repo, not in the published crate
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/contrib/extractor/matrix.dat");
        let Ok(matrix) = std::fs::read(path) else {
            return;
        };
        let matrix = Matrix::new(&matrix, 1024, 768).unwrap();
        // 2 blocks, bytes 0x00..=0xff
        let input: Vec<u8> = (0..=255).collect();
        let mut output = [0u8; 192];
        matrix.extract(&input, &mut output);

        // `QuantisExtractorProcessBlock` with the same matrix and input
        let expected = [
            "3f522cb5e7adda2f5d36f0f163364b8faabefab0ce9193025c5b04a148008d51",
            "3b760784952a187b6204e8ea53a56ca6a2c1e2fbd7417837633c86bdb1518834",
            "2252dd4cec4bc21257ab41ab8b2bcfd7f232dffa5f3ed3935745d20ba7fa87e0",
            "c8decf60e8a4ae9e8dd7e756b15c2d704004330b5222a497d47fedd9aa4f94a0",
            "e8bcdbc8c80c10d897387c1499af6b0c5fae1004b88643d1337a568c51d3bee4",
            "486a79bcc488bc1ad672f5daaa5d6aaf5268155be90ff85dc2d4a6c7916fd0e1",
        ]
        .concat();
        let mut hex = String::new();
        for b in output {
            write!(hex, "{b:02x}").unwrap();
        }
        assert_eq!(hex, expected);
    }

    #[test]
    fn test_rows_and_bits_order() {
        // 128x64, row `r` -> bit `r` of the first input word
        let mut rows = Vec::new();
        for r in 0..64 {
            rows.extend_from_slice(&(1u64 << r).to_le_bytes());
            rows.extend_from_slice(&0u64.to_le_bytes());
        }
        let matrix = Matrix::new(&rows, 128, 64).unwrap();

        // the first word is copied, the second one is ignored
        let mut input = [0u8; 16];
        input[..8].copy_from_slice(&0x0123_4567_89ab_cdefu64.to_le_bytes());
        input[8..].fill(0xff);
        let mut output = [0u8; 8];
        matrix.extract(&input, &mut output);
        assert_eq!(u64::from_le_bytes(output), 0x0123_4567_89ab_cdef);
    }

    #[test]
    fn test_parity() {
        // 128x64, all ones -> every output bit is the parity of the input
        let matrix = Matrix::new(&[0xff; 1024], 128, 64).unwrap();
        let mut output = [0u8; 8];

        let mut input = [0u8; 16];
        input[3] = 0b0000_0111;
        matrix.extract(&input, &mut output);
        assert_eq!(output, [0xff; 8]);

        input[15] = 0b1000_0000;
        matrix.extract(&input, &mut output);
        assert_eq!(output, [0; 8]);
    }

    #[test]
    fn test_longer_matrix() {
        // only the first 128 * 64 / 8 bytes are used
        let matrix: Vec<u8> = (0..=255).cycle().take(2048).collect();
        let a = Matrix::new(&matrix[..1024], 128, 64).unwrap();
        let b = Matrix::new(&matrix, 128, 64).unwrap();
        let input: Vec<u8> = (0..16).collect();
        let (mut out_a, mut out_b) = ([0u8; 8], [0u8; 8]);
        a.extract(&input, &mut out_a);
        b.extract(&input, &mut out_b);
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn test_new_rejects_invalid() {
        let matrix: &[u8] = &vec![0; 1024 * 768 / 8];
        let cases = [
            (matrix, 1000, 768),
            (matrix, 1024, 700),
            (matrix, 1024, 0),
            (matrix, 1024, 1024),
            (matrix, 768, 1024),
            (&matrix[..1000], 1024, 768),
            // overflow of `size_in * size_out`
            (matrix, usize::MAX - 63, 64),
        ];
        for (matrix, size_in, size_out) in cases {
            assert!(
                matches!(
                    Matrix::new(matrix, size_in, size_out),
                    Err(Error::InvalidMatrix(_))
                ),
                "{size_in}x{size_out} with {} bytes accepted",
                matrix.len()
            );
        }
    }

    #[test]
    #[should_panic = "whole blocks"]
    fn test_extract_partial_block() {
        // 128x64 -> blocks of 16 bytes in, 8 bytes out
        let matrix = Matrix::new(&[0; 1024], 128, 64).unwrap();
        matrix.extract(&[0; 20], &mut [0; 8]);
    }

    #[test]
    #[should_panic = "whole blocks"]
    fn test_extract_different_blocks() {
        let matrix = Matrix::new(&[0; 1024], 128, 64).unwrap();
        matrix.extract(&[0; 32], &mut [0; 8]);
    }
}
