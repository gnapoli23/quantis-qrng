# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Before 1.0 a minor release (e.g. 0.1 -> 0.2) can have breaking changes.

## 0.1.0-alpha.2 - 2026-11-08

### Added

- `io::Read for QuantisUsb` and `From<Error> for io::Error`
- `rand` support, randomness extraction and numbers in `qrng read`

### Documentation

- Update README with device access and function behavior

## 0.1.0-alpha.1 - 2026-11-06

First pre-release, Quantis USB only. The vendor commands work on real
hardware, reading random data (`fill_bytes`, `qrng read`) still has to be
tested on a real device.

### Added

- `QuantisUsb`: open the device (`open`, `open_serial`), list the attached
  devices without opening them (`list`) and read random data (`fill_bytes`).
  `fill_bytes` fills the whole buffer or returns an error, and on error the
  buffer is set to zero.
- Vendor commands: board version, module mask, status, power and rate, AIS 31
  start-up tests flag, enable and disable the module.
- `qrng` command-line tool (`cli` feature): `list`, `probe` and `read`, with
  `--serial`, `--hex` and `-o FILE`.
- udev rule for Linux in `contrib/udev`.
- Linux support. macOS and Windows build, but are not tested yet (Windows
  needs the WinUSB driver).

### Fixed

Bugs of the original ID Quantique library that are not in this crate (see the
README):

- a short USB transfer ended the read with the buffer only partially filled,
  and `EasyQuantis` wrote it out padded with zeros;
- the device index check was off by one and read uninitialized memory.
