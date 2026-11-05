//! `qrng`.
//!
//! Available commands:
//! - `probe`: opens a connection with the Quantis USB device (if any connected) and show manufacturer and serial info

use clap::{Parser, Subcommand};
use quantis_qrng::{Error, QuantisUsb};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Probe,
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Probe => {
            // open device
            let mut quantis = QuantisUsb::open()?;

            // collect data
            let manufacturer = quantis.manufacturer().unwrap_or_default().to_string();
            let serial = quantis.serial_number().unwrap_or_default().to_string();
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
            println!("Board version: {board_version}");
            println!("Modules status: {modules_status}");
            println!("Modules power: {modules_power}");
            println!("Modules rate: {modules_rate}");
            println!("Module mask: {module_mask}");
        }
    }

    Ok(())
}
