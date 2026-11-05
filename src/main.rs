//! `qrng`: command-line tool for Quantis quantum random number generators.
//!
//! Commands:
//!
//! * `probe`: opens the Quantis USB device and prints its identity and the
//!   value of every register. It only reads: the device state is not changed.

use std::{error::Error as _, process::ExitCode};

use clap::{Parser, Subcommand};
use quantis_qrng::{Error, usb::QuantisUsb};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the identity and the registers of the attached Quantis USB.
    Probe,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&err);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<(), Error> {
    match cli.command {
        Commands::Probe => probe(),
    }
}

fn probe() -> Result<(), Error> {
    // open device
    let mut quantis = QuantisUsb::open()?;

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

/// Prints `err`, the chain of its causes and, when there is one, a hint.
fn report(err: &Error) {
    eprintln!("qrng: error: {err}");

    let mut source = err.source();
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }

    let hint = match err {
        Error::PermissionDenied => Some(
            "on Linux, install a udev rule for 0aba:0102 and make sure your user is in its group",
        ),
        Error::Busy => Some("stop the other program that is using the device"),
        Error::NoDevice => Some("check the cable and `lsusb -d 0aba:0102`"),
        _ => None,
    };
    if let Some(hint) = hint {
        eprintln!("  hint: {hint}");
    }
}
