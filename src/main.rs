//! `qrng`: command-line tool for Quantis quantum random number generators.
//!
//! Commands:
//!
//! * `list`: lists the attached Quantis USB devices, without opening them.
//! * `probe`: opens the Quantis USB device and prints its identity and the
//!   value of every register. It only reads: the device state is not changed.
//! * `read`: reads random bytes from the Quantis USB device and writes them to
//!   standard output, raw or as hexadecimal text.
//!
//! `probe` and `read` open the only attached device, or the one given with
//! `--serial`.

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
    /// The devices are not opened: this works while another program is using
    /// them, and shows only what the operating system knows. Use `probe` for
    /// the registers of a device.
    List,
    /// Print the identity and the registers of the attached Quantis USB.
    Probe(DeviceArgs),
    /// Read random bytes from the attached Quantis USB and write them to
    /// standard output.
    ///
    /// The output is raw binary data, meant to be redirected to a file or
    /// piped to another program; use --hex to print it on a terminal. If an
    /// error stops the read, the output is truncated (it never contains
    /// bytes that did not come from the device) and the exit status is 1.
    Read(ReadArgs),
}

#[derive(Args)]
struct DeviceArgs {
    /// Serial number of the device to open (see `qrng list`). Required when
    /// more than one Quantis USB is attached.
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

    /// Number of bytes to read. Accepts the suffixes K, M and G (powers of
    /// 1024), e.g. 4K or 10M. There is no upper limit: the data is streamed.
    #[arg(short = 'n', long = "size", default_value = "1024", value_parser = parse_size)]
    size: u64,

    /// Write the bytes as lowercase hexadecimal text, 32 bytes per line.
    #[arg(long)]
    hex: bool,
}

/// Bytes read from the device and written to the output at a time.
///
/// Like the 8 KiB chunks of `EasyQuantis`, it keeps memory use constant
/// whatever the requested size. It is a multiple of 32, so every hex line but
/// the last one is complete.
const OUTPUT_CHUNK_SIZE: usize = 64 * 1024;

/// Error of a `qrng` command.
#[derive(Debug)]
enum CliError {
    /// The device or the driver failed.
    Device(Error),
    /// Writing the output failed.
    Output(io::Error),
    /// Raw binary output was requested on a terminal.
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
        // the reader went away (e.g. `qrng read | head -c 16`): not an error
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
    // checked before opening the device: nothing is read for nothing
    if !args.hex && stdout.is_terminal() {
        return Err(CliError::BinaryToTerminal);
    }

    let mut quantis = args.device.open()?;
    let mut out = BufWriter::with_capacity(OUTPUT_CHUNK_SIZE, stdout.lock());
    let mut buf = vec![0u8; OUTPUT_CHUNK_SIZE];
    let mut remaining = args.size;

    while remaining > 0 {
        // `remaining` may not fit in a usize on 32-bit targets
        let len =
            usize::try_from(remaining).map_or(OUTPUT_CHUNK_SIZE, |r| r.min(OUTPUT_CHUNK_SIZE));
        let chunk = &mut buf[..len];

        // only data from a successful read is written: on error `read` has
        // wiped `chunk` and nothing of it reaches the output
        quantis.read(chunk)?;
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

/// Writes `data` as lowercase hexadecimal, 32 bytes per line, each line
/// ending with a newline.
fn write_hex(out: &mut impl Write, data: &[u8]) -> io::Result<()> {
    for line in data.chunks(32) {
        for byte in line {
            write!(out, "{byte:02x}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// Parses a byte count: decimal digits, optionally followed by `K`, `M` or
/// `G` (powers of 1024, case-insensitive).
fn parse_size(s: &str) -> Result<u64, String> {
    let (digits, multiplier) = match s.as_bytes().last() {
        Some(b'K' | b'k') => (&s[..s.len() - 1], 1u64 << 10),
        Some(b'M' | b'm') => (&s[..s.len() - 1], 1 << 20),
        Some(b'G' | b'g') => (&s[..s.len() - 1], 1 << 30),
        _ => (s, 1),
    };

    // `u64::from_str` also accepts a leading `+`: only plain digits here
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

/// Prints `err`, the chain of its causes and, when there is one, a hint.
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
            "on Linux, install a udev rule for 0aba:0102 and make sure your user is in its group",
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
        // overflow, in the digits or in the multiplier
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
        assert!(out.is_empty());
    }
}
