//! `qrng`, command-line tool for Quantis quantum random number generators.
//!
//! * `list` prints the attached Quantis USB devices, without opening them.
//! * `probe` prints info and registers of a device. It only reads, nothing is
//!   changed on the device.
//! * `read` writes random bytes to stdout or to a file, raw or as hex, or
//!   integers and floats in a range.
//!
//! `probe` and `read` open the only attached device, or the one with the
//! serial number given with `--serial`.

use std::{
    error::Error as _,
    fmt,
    fs::{self, File},
    io::{self, BufWriter, IsTerminal as _, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand, error::ErrorKind};
use quantis_qrng::{
    Error,
    extractor::Matrix,
    usb::{QuantisRng, QuantisUsb},
};
use rand_core::TryRng;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List the attached Quantis USB devices and their serial numbers.
    ///
    /// The devices are not opened, so it works even if another program is
    /// using them. Use `probe` to read the registers of a device.
    List,
    /// Print the identity and the registers of the attached Quantis USB.
    Probe(DeviceArgs),
    /// Read random bytes from the attached Quantis USB and write them to
    /// stdout.
    ///
    /// The output is binary, redirect it to a file or pipe it to another
    /// program, or use --hex, --integers or --floats to print it on the
    /// terminal. On error the output stops (it only contains data from the
    /// device) and the exit status is 1.
    Read(ReadArgs),
}

#[derive(Args)]
struct DeviceArgs {
    /// Serial number of the device to open (see `qrng list`). Needed if more
    /// than one Quantis USB is attached.
    #[arg(short, long)]
    serial: Option<String>,
}

#[derive(Args)]
struct ReadArgs {
    #[command(flatten)]
    device: DeviceArgs,

    /// Number of bytes to read (numbers with --integers or --floats), with
    /// optional suffix K, M or G (powers of 1024), e.g. 4K or 10M. There is no
    /// upper limit.
    #[arg(short = 'n', long = "size", default_value = "1024", value_parser = parse_size)]
    size: u64,

    /// Print the bytes as hex, 32 bytes per line.
    #[arg(long, conflicts_with = "numbers")]
    hex: bool,

    /// Write integers, one per line, from --min to --max (both included).
    /// Without them, any 32-bit signed integer.
    #[arg(long, group = "numbers")]
    integers: bool,

    /// Write floats, one per line, from --min (included) to --max (excluded).
    /// Without them, from 0 to 1.
    #[arg(long, group = "numbers")]
    floats: bool,

    /// Smallest number, with --integers or --floats.
    #[arg(long, requires_all = ["max", "numbers"], allow_negative_numbers = true)]
    min: Option<String>,

    /// Largest number, with --integers or --floats.
    #[arg(long, requires_all = ["min", "numbers"], allow_negative_numbers = true)]
    max: Option<String>,

    /// Extract the bytes with the matrix in FILE, like `EasyQuantis -m` (e.g.
    /// contrib/extractor/matrix.dat). It removes the bias of the raw
    /// data, every block of --matrix-size-in bits becomes --matrix-size-out
    /// bits.
    #[arg(short, long, value_name = "FILE", conflicts_with = "numbers")]
    matrix: Option<PathBuf>,

    /// Input bits of the matrix, multiple of 64.
    #[arg(long, default_value_t = 1024, requires = "matrix")]
    matrix_size_in: usize,

    /// Output bits of the matrix, multiple of 64 and lower than
    /// --matrix-size-in.
    #[arg(long, default_value_t = 768, requires = "matrix")]
    matrix_size_out: usize,

    /// Write the bytes to FILE instead of stdout. The file is created, or
    /// overwritten if it exists. On Windows use it instead of `>`, the shell
    /// can change binary data.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
}

// output of --integers and --floats
#[derive(Clone, Copy)]
enum Numbers {
    Integers(i64, i64),
    Floats(f64, f64),
}

// bytes read and written at a time, so the memory used is the same for any
// size (EasyQuantis uses 8 KiB). multiple of 32 -> only the last hex line can
// be shorter
const OUTPUT_CHUNK_SIZE: usize = 64 * 1024;

// error of a command
#[derive(Debug)]
enum CliError {
    // device or driver error
    Device(Error),
    // can't write the output
    Output(io::Error),
    // can't create the output file
    CreateOutput(PathBuf, io::Error),
    // can't read the matrix file
    ReadMatrix(PathBuf, io::Error),
    // binary output on a terminal
    BinaryToTerminal,
}

impl From<Error> for CliError {
    fn from(value: Error) -> Self {
        CliError::Device(value)
    }
}

impl From<io::Error> for CliError {
    fn from(value: io::Error) -> Self {
        CliError::Output(value)
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::Device(err) => err.fmt(f),
            CliError::Output(err) => write!(f, "cannot write the output: {err}"),
            CliError::CreateOutput(path, err) => {
                write!(f, "cannot create {}: {err}", path.display())
            }
            CliError::ReadMatrix(path, err) => {
                write!(f, "cannot read {}: {err}", path.display())
            }
            CliError::BinaryToTerminal => {
                write!(f, "refusing to write binary data to a terminal")
            }
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match &cli.command {
        Commands::List => list(),
        Commands::Probe(args) => probe(args),
        Commands::Read(args) => read(args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        // the reader is gone (e.g. `qrng read | head -c 16`), not an error
        Err(CliError::Output(err)) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            report(&err);
            ExitCode::FAILURE
        }
    }
}

fn list() -> Result<(), CliError> {
    let devices = QuantisUsb::list()?;
    if devices.is_empty() {
        println!("No Quantis USB device found.");
        return Ok(());
    }

    for (i, device) in devices.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("Serial: {}", device.serial_number().unwrap_or("N/A"));
        println!("  Manufacturer: {}", device.manufacturer().unwrap_or("N/A"));
        println!("  Product: {}", device.product().unwrap_or("N/A"));
        println!(
            "  Bus {} Device {:03}",
            device.bus_id(),
            device.device_address()
        );
    }

    Ok(())
}

fn probe(args: &DeviceArgs) -> Result<(), CliError> {
    // open device
    let mut quantis = match &args.serial {
        Some(serial) => QuantisUsb::open_serial(serial)?,
        None => QuantisUsb::open()?,
    };

    // collect data
    let manufacturer = quantis.manufacturer().unwrap_or("N/A").to_string();
    let serial = quantis.serial_number().unwrap_or("N/A").to_string();
    let max_packet_size = quantis.max_packet_size();
    let board_version = quantis.board_version()?;
    let modules_status = quantis.modules_status()?;
    let modules_power = quantis.modules_power()?;
    let modules_rate = quantis.modules_rate()?;
    let module_mask = quantis.module_mask()?;

    // show data
    println!("Device: Quantis QRNG (USB)");
    println!("Manufacturer: {manufacturer}");
    println!("Serial: {serial}");
    println!("Max packet size: {max_packet_size}");
    println!("Board version: {board_version:#010x}");
    println!("Modules status: {modules_status}");
    println!("Modules power: {modules_power}");
    println!("Modules rate: {modules_rate}");
    println!("Module mask: {module_mask}");

    Ok(())
}

fn read(args: &ReadArgs) -> Result<(), CliError> {
    // same exit as the other usage errors
    let numbers = numbers(args)
        .unwrap_or_else(|msg| clap::Error::raw(ErrorKind::ValueValidation, msg + "\n").exit());
    let binary = !args.hex && numbers.is_none();

    // before opening the device, like the checks on stdout
    let matrix = match &args.matrix {
        Some(path) => {
            let matrix = fs::read(path).map_err(|err| CliError::ReadMatrix(path.clone(), err))?;
            Some(Matrix::new(
                &matrix,
                args.matrix_size_in,
                args.matrix_size_out,
            )?)
        }
        None => None,
    };

    // checks on stdout, before opening the device
    if args.output.is_none() {
        if binary && io::stdout().is_terminal() {
            return Err(CliError::BinaryToTerminal);
        }
        // on Windows the shell can re-encode binary data (Windows PowerShell
        // converts it to UTF-16), text is fine
        if cfg!(windows) && binary {
            eprintln!(
                "qrng: warning: on Windows the shell can change binary data redirected with `>` or `|` \
                 (Windows PowerShell does), use `-o FILE` to write it safely"
            );
        }
    }

    let mut quantis = match &args.device.serial {
        Some(serial) => QuantisUsb::open_serial(serial)?,
        None => QuantisUsb::open()?,
    };

    // the file is created only after the device is open, so an existing file is
    // not overwritten if the device can't be used
    let out: Box<dyn Write> = match &args.output {
        Some(path) => {
            Box::new(File::create(path).map_err(|err| CliError::CreateOutput(path.clone(), err))?)
        }
        None => Box::new(io::stdout().lock()),
    };
    let mut out = BufWriter::with_capacity(OUTPUT_CHUNK_SIZE, out);

    if let Some(numbers) = numbers {
        let mut rng = QuantisRng::new(quantis);
        for _ in 0..args.size {
            match numbers {
                Numbers::Integers(min, max) => {
                    writeln!(out, "{}", random_integer(&mut rng, min, max)?)?;
                }
                Numbers::Floats(min, max) => {
                    writeln!(out, "{}", random_float(&mut rng, min, max)?)?;
                }
            }
        }
        out.flush()?;
        return Ok(());
    }

    // with --matrix whole blocks are extracted, the rest of the last one is
    // thrown away
    let (bytes_in, bytes_out) = (args.matrix_size_in / 8, args.matrix_size_out / 8);
    let blocks = matrix
        .as_ref()
        .map_or(0, |_| OUTPUT_CHUNK_SIZE.div_ceil(bytes_out));
    let mut buf = vec![0u8; OUTPUT_CHUNK_SIZE.max(blocks * bytes_out)];
    let mut raw = vec![0u8; blocks * bytes_in];
    let mut remaining = args.size;

    while remaining > 0 {
        // `remaining` could not fit in a usize on 32-bit targets
        let len =
            usize::try_from(remaining).map_or(OUTPUT_CHUNK_SIZE, |r| r.min(OUTPUT_CHUNK_SIZE));

        // write only after a successful read, on error nothing of `chunk` is written
        match &matrix {
            Some(matrix) => {
                let blocks = len.div_ceil(bytes_out);
                let raw = &mut raw[..blocks * bytes_in];
                quantis.fill_bytes(raw)?;
                matrix.extract(raw, &mut buf[..blocks * bytes_out]);
            }
            None => quantis.fill_bytes(&mut buf[..len])?,
        }
        let chunk = &buf[..len];
        if args.hex {
            write_hex(&mut out, chunk)?;
        } else {
            out.write_all(chunk)?;
        }

        remaining -= len as u64; // len <= remaining
    }

    out.flush()?;
    Ok(())
}

// bytes per line of the hex output
const HEX_LINE: usize = 32;

// every chunk starts on a new hex line
const _: () = assert!(OUTPUT_CHUNK_SIZE % HEX_LINE == 0);

// writes `data` as lowercase hex, 32 bytes per line
fn write_hex(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    let mut hex = [0u8; HEX_LINE * 2];
    for line in data.chunks(HEX_LINE) {
        let hex = &mut hex[..line.len() * 2];
        // it only fails if the output length is wrong, and here it is right
        const_hex::encode_to_slice(line, hex).map_err(io::Error::other)?;
        out.write_all(hex)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

// --integers or --floats with checked bounds, defaults of EasyQuantis
fn numbers(args: &ReadArgs) -> Result<Option<Numbers>, String> {
    let (min, max) = (args.min.as_deref(), args.max.as_deref());

    if args.integers {
        let parse = |s: &str| {
            s.parse::<i64>()
                .map_err(|_| format!("`{s}` is not a 64-bit integer"))
        };
        let min = min.map_or(Ok(i32::MIN.into()), parse)?;
        let max = max.map_or(Ok(i32::MAX.into()), parse)?;
        if min >= max {
            return Err("--min must be lower than --max".into());
        }
        Ok(Some(Numbers::Integers(min, max)))
    } else if args.floats {
        let parse = |s: &str| match s.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(x),
            _ => Err(format!("`{s}` is not a finite number")),
        };
        let min = min.map_or(Ok(0.0), parse)?;
        let max = max.map_or(Ok(1.0), parse)?;
        if min >= max {
            return Err("--min must be lower than --max".into());
        }
        // `max - min` is used for the scaling
        if !(max - min).is_finite() {
            return Err("the range from --min to --max is too large".into());
        }
        Ok(Some(Numbers::Floats(min, max)))
    } else {
        Ok(None)
    }
}

// integer in `min..=max`, no modulo bias -> the highest 2^64 % range values
// are thrown away
fn random_integer<R: TryRng>(rng: &mut R, min: i64, max: i64) -> Result<i64, R::Error> {
    // 0 -> all the 2^64 values
    let range = max.abs_diff(min).wrapping_add(1);
    let limit = match range {
        0 => u64::MAX,
        _ => u64::MAX - (u64::MAX % range + 1) % range,
    };
    loop {
        let x = rng.try_next_u64()?;
        if x <= limit {
            return Ok(min.wrapping_add_unsigned(x.checked_rem(range).unwrap_or(x)));
        }
    }
}

// float in `min..max`
fn random_float<R: TryRng>(rng: &mut R, min: f64, max: f64) -> Result<f64, R::Error> {
    loop {
        // 53 bits -> [0, 1), exact
        #[expect(clippy::cast_precision_loss)]
        let unit = (rng.try_next_u64()? >> 11) as f64 / (1u64 << 53) as f64;
        // rounding can give `max` -> drawn again (`ConvertToFloat_01` can return 1.0)
        let x = min + unit * (max - min);
        if x < max {
            return Ok(x);
        }
    }
}

// parses a size: digits with optional K, M or G (powers of 1024, also lowercase)
fn parse_size(s: &str) -> Result<u64, String> {
    let (digits, multiplier) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1 << 20),
        Some(b'G' | b'g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };

    // `u64::from_str` also accepts a leading `+`, we only want digits
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "invalid size `{s}`: expected a number of bytes, optionally followed by K, M or G"
        ));
    }

    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(multiplier))
        .ok_or_else(|| format!("size `{s}` is too large"))
}

// prints the error, its causes and a hint if there is one
fn report(err: &CliError) {
    eprintln!("qrng: error: {err}");

    let mut source = match err {
        CliError::Device(err) => err.source(),
        CliError::Output(err) | CliError::CreateOutput(_, err) | CliError::ReadMatrix(_, err) => {
            err.source()
        }
        CliError::BinaryToTerminal => None,
    };
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }

    let hint = match err {
        CliError::Device(Error::PermissionDenied) => Some(
            "on Linux, install the udev rule 70-quantis-qrng.rules (see the README) and make sure your user is in the `plugdev` group",
        ),
        CliError::Device(Error::Busy) => Some("stop the other program that is using the device"),
        CliError::Device(Error::NoDevice) => Some("check the cable and `lsusb -d 0aba:0102`"),
        CliError::Device(Error::TooManyDevices) => {
            Some("choose one with --serial; `qrng list` shows the serial numbers")
        }
        CliError::Device(Error::SerialNotFound(_)) => {
            Some("`qrng list` shows the serial numbers of the attached devices")
        }
        CliError::Device(Error::Unsupported(_)) if cfg!(windows) => Some(
            "the device must use the WinUSB driver instead of the ID Quantique one (see the README)",
        ),
        CliError::Device(Error::ModuleNotReady) => {
            Some("check `Modules status` with `qrng probe`; re-plug the device if it stays 0")
        }
        CliError::BinaryToTerminal => Some("use --hex, or redirect the output to a file or a pipe"),
        _ => None,
    };
    if let Some(hint) = hint {
        eprintln!("  hint: {hint}");
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, fmt::Write as _};

    use clap::Parser as _;
    use rand_core::TryRng;

    use super::{
        Cli, Commands, HEX_LINE, Numbers, numbers, parse_size, random_float, random_integer,
        write_hex,
    };

    // returns the given values
    struct Values<const N: usize>(std::array::IntoIter<u64, N>);

    impl<const N: usize> TryRng for Values<N> {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Infallible> {
            unreachable!()
        }

        fn try_next_u64(&mut self) -> Result<u64, Infallible> {
            Ok(self.0.next().unwrap())
        }

        fn try_fill_bytes(&mut self, _dst: &mut [u8]) -> Result<(), Infallible> {
            unreachable!()
        }
    }

    #[test]
    fn test_numbers() {
        let cli = Cli::try_parse_from(["qrng", "read"]).unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(numbers(&args), Ok(None)));

        let cli = Cli::try_parse_from(["qrng", "read", "--hex"]).unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(numbers(&args), Ok(None)));

        // EasyQuantis defaults
        let cli = Cli::try_parse_from(["qrng", "read", "--integers"]).unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(
            numbers(&args),
            Ok(Some(Numbers::Integers(-2_147_483_648, 2_147_483_647)))
        ));

        let cli = Cli::try_parse_from(["qrng", "read", "--floats"]).unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(
            numbers(&args),
            Ok(Some(Numbers::Floats(0.0, 1.0)))
        ));

        let cli = Cli::try_parse_from(["qrng", "read", "--integers", "--min", "-5", "--max", "5"])
            .unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(numbers(&args), Ok(Some(Numbers::Integers(-5, 5)))));

        let cli =
            Cli::try_parse_from(["qrng", "read", "--floats", "--min", "-0.5", "--max", "1e3"])
                .unwrap();
        let Commands::Read(args) = cli.command else {
            unreachable!()
        };
        assert!(matches!(
            numbers(&args),
            Ok(Some(Numbers::Floats(-0.5, 1000.0)))
        ));
    }

    #[test]
    fn test_numbers_rejects_invalid() {
        for args in [
            "--integers --min 6 --max 1",
            "--integers --min 1 --max 1",
            "--integers --min 1.5 --max 6",
            "--integers --min 1 --max 9223372036854775808",
            "--floats --min 1 --max 1",
            "--floats --min inf --max 1",
            "--floats --min 0 --max NaN",
            "--floats --min -1e308 --max 1e308",
        ] {
            let cli = Cli::try_parse_from(format!("qrng read {args}").split_whitespace()).unwrap();
            let Commands::Read(read) = cli.command else {
                unreachable!()
            };
            assert!(numbers(&read).is_err(), "`{args}` accepted");
        }
    }

    #[test]
    fn test_random_integer_bounds() {
        let cases = [
            (-5, 5, 0, -5),
            (-5, 5, 10, 5),
            // power of 2 -> nothing thrown away
            (0, 255, u64::MAX, 255),
            (i64::MIN, i64::MAX, 0, i64::MIN),
            (i64::MIN, i64::MAX, u64::MAX, i64::MAX),
        ];
        for (min, max, x, expected) in cases {
            let n = random_integer(&mut Values([x].into_iter()), min, max).unwrap();
            assert_eq!(n, expected, "{min}..={max} with {x}");
        }
    }

    #[test]
    fn test_random_integer_no_modulo_bias() {
        // 2^64 % 6 = 4 -> the 4 highest values are thrown away
        let mut rng = Values([u64::MAX, u64::MAX - 3, u64::MAX - 4].into_iter());
        // (2^64 - 5) % 6 = 5
        assert_eq!(random_integer(&mut rng, 1, 6).unwrap(), 6);
        assert_eq!(rng.0.len(), 0);
    }

    #[test]
    #[allow(clippy::float_cmp)] // exact values
    fn test_random_float_bounds() {
        assert_eq!(
            random_float(&mut Values([0].into_iter()), 0.0, 1.0).unwrap(),
            0.0
        );
        let x = random_float(&mut Values([u64::MAX].into_iter()), 0.0, 1.0).unwrap();
        assert_eq!(x, 1.0 - f64::EPSILON / 2.0);
        assert_eq!(
            random_float(&mut Values([0].into_iter()), -3.0, 2.0).unwrap(),
            -3.0
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // exact values
    fn test_random_float_never_max() {
        // 1e16 and 1e16 + 2 are next to each other -> the highest value rounds to `max`
        let mut rng = Values([u64::MAX, 0].into_iter());
        assert_eq!(random_float(&mut rng, 1e16, 1e16 + 2.0).unwrap(), 1e16);
        assert_eq!(rng.0.len(), 0);
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size("1100"), Ok(1100));
        assert_eq!(parse_size("4K"), Ok(4096));
        assert_eq!(parse_size("4k"), Ok(4096));
        assert_eq!(parse_size("10M"), Ok(10 * 1024 * 1024));
        assert_eq!(parse_size("1G"), Ok(1 << 30));
        assert_eq!(parse_size("18446744073709551615"), Ok(u64::MAX));
    }

    #[test]
    fn test_parse_size_rejects_invalid() {
        for s in ["", "K", "+5", "-5", "1.5M", "5 K", "5KB", "0x10", "ten"] {
            assert!(parse_size(s).is_err(), "`{s}` accepted");
        }
        // overflow of the digits or of the multiplier
        assert!(parse_size("18446744073709551616").is_err());
        assert!(parse_size("17179869184G").is_err());
    }

    #[test]
    fn test_write_hex() {
        let mut out = Vec::new();
        let data: Vec<u8> = (0..40).collect();
        write_hex(&mut out, &data).unwrap();

        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), HEX_LINE * 2);
        assert!(lines[0].starts_with("000102"));
        assert_eq!(lines[1], "2021222324252627");
        assert!(text.ends_with('\n'));

        let mut out = Vec::new();
        write_hex(&mut out, &[]).unwrap();
        assert_eq!(out, Vec::<u8>::new());
    }

    #[test]
    fn test_write_hex_all_bytes() {
        // same output as `format!("{:02x}")` for every byte value
        let data: Vec<u8> = (0..=255).collect();
        let mut expected = String::new();
        for line in data.chunks(HEX_LINE) {
            for b in line {
                write!(expected, "{b:02x}").unwrap();
            }
            expected.push('\n');
        }

        let mut out = Vec::new();
        write_hex(&mut out, &data).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), expected);
    }
}
