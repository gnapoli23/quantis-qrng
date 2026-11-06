//! `qrng`, command-line tool for Quantis quantum random number generators.
//!
//! * `list` prints the attached Quantis USB devices, without opening them.
//! * `probe` prints info and registers of a device. It only reads, nothing is
//!   changed on the device.
//! * `read` writes random bytes to stdout, raw or as hex.
//!
//! `probe` and `read` open the only attached device, or the one with the
//! serial number given with `--serial`.

use std::{
    error::Error as _,
    fmt,
    io::{self, BufWriter, IsTerminal as _, Write},
    process::ExitCode,
};

use clap::{Args, Parser, Subcommand};
use quantis_qrng::{Error, usb::QuantisUsb};

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
    /// program, or use --hex to print it on the terminal. On error the output
    /// stops (it only contains bytes from the device) and the exit status
    /// is 1.
    Read(ReadArgs),
}

#[derive(Args)]
struct DeviceArgs {
    /// Serial number of the device to open (see `qrng list`). Needed if more
    /// than one Quantis USB is attached.
    #[arg(short, long)]
    serial: Option<String>,
}

impl DeviceArgs {
    fn open(&self) -> Result<QuantisUsb, Error> {
        match &self.serial {
            Some(serial) => QuantisUsb::open_serial(serial),
            None => QuantisUsb::open(),
        }
    }
}

#[derive(Args)]
struct ReadArgs {
    #[command(flatten)]
    device: DeviceArgs,

    /// Number of bytes to read, with optional suffix K, M or G (powers of
    /// 1024), e.g. 4K or 10M. There is no upper limit.
    #[arg(short = 'n', long = "size", default_value = "1024", value_parser = parse_size)]
    size: u64,

    /// Print the bytes as hex, 32 bytes per line.
    #[arg(long)]
    hex: bool,
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
            CliError::BinaryToTerminal => {
                write!(f, "refusing to write binary data to a terminal")
            }
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        // the reader is gone (e.g. `qrng read | head -c 16`), not an error
        Err(CliError::Output(err)) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(err) => {
            report(&err);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    match &cli.command {
        Commands::List => list(),
        Commands::Probe(args) => probe(args),
        Commands::Read(args) => read(args),
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
    let mut quantis = args.open()?;

    // collect data
    let manufacturer = quantis.manufacturer().unwrap_or("N/A").to_string();
    let serial = quantis.serial_number().unwrap_or("N/A").to_string();
    let max_packet_size = quantis.max_packet_size();
    let board_version = quantis.board_version()?;
    let modules_status = quantis.modules_status()?;
    let modules_power = quantis.modules_power()?;
    let modules_rate = quantis.modules_rate()?;
    let module_mask = quantis.module_mask()?;
    let ais31_flag = quantis.ais31_startup_tests_request_flag()?;

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
    println!("AIS-31 flag: {ais31_flag}");

    Ok(())
}

fn read(args: &ReadArgs) -> Result<(), CliError> {
    let stdout = io::stdout();
    // check it before opening the device
    if !args.hex && stdout.is_terminal() {
        return Err(CliError::BinaryToTerminal);
    }

    let mut quantis = args.device.open()?;
    let mut out = BufWriter::with_capacity(OUTPUT_CHUNK_SIZE, stdout.lock());
    let mut buf = vec![0u8; OUTPUT_CHUNK_SIZE];
    let mut remaining = args.size;

    while remaining > 0 {
        // `remaining` could not fit in a usize on 32-bit targets
        let len =
            usize::try_from(remaining).map_or(OUTPUT_CHUNK_SIZE, |r| r.min(OUTPUT_CHUNK_SIZE));
        let chunk = &mut buf[..len];

        // write only after a successful read, on error nothing of `chunk` is written
        quantis.fill_bytes(chunk)?;
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

// writes `data` as lowercase hex, 32 bytes per line
fn write_hex(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    for line in data.chunks(32) {
        for byte in line {
            write!(out, "{byte:02x}")?;
        }
        writeln!(out)?;
    }
    Ok(())
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
        CliError::Output(err) => err.source(),
        CliError::BinaryToTerminal => None,
    };
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }

    let hint = match err {
        CliError::Device(Error::PermissionDenied) => Some(
            "on Linux, install contrib/udev/70-quantis-qrng.rules (see the README) and make sure your user is in the `plugdev` group",
        ),
        CliError::Device(Error::Busy) => Some("stop the other program that is using the device"),
        CliError::Device(Error::NoDevice) => Some("check the cable and `lsusb -d 0aba:0102`"),
        CliError::Device(Error::TooManyDevices) => {
            Some("choose one with --serial; `qrng list` shows the serial numbers")
        }
        CliError::Device(Error::SerialNotFound(_)) => {
            Some("`qrng list` shows the serial numbers of the attached devices")
        }
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
    use super::{parse_size, write_hex};

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
        assert_eq!(lines[0].len(), 64);
        assert!(lines[0].starts_with("000102"));
        assert_eq!(lines[1], "2021222324252627");
        assert!(text.ends_with('\n'));

        let mut out = Vec::new();
        write_hex(&mut out, &[]).unwrap();
        assert_eq!(out, Vec::<u8>::new());
    }
}
