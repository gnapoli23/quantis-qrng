//! Finding the Quantis USB devices attached to the system.

use crate::error::Error;

/// A Quantis USB device attached to the system, as seen by the operating
/// system, without opening it.
///
/// Returned by [`QuantisUsb::list`](crate::usb::QuantisUsb::list). Getting it
/// sends no request to the device and does not claim it, so it works while
/// another process is using the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub(crate) serial_number: Option<String>,
    pub(crate) manufacturer: Option<String>,
    pub(crate) product: Option<String>,
    pub(crate) bus_id: String,
    pub(crate) device_address: u8,
}

impl DeviceInfo {
    /// Serial number of the device, if the operating system provides it.
    ///
    /// It identifies the device: use it with
    /// [`QuantisUsb::open_serial`](crate::usb::QuantisUsb::open_serial).
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// Manufacturer of the device, if the operating system provides it.
    ///
    /// Always `None` on Windows, which does not cache this string.
    #[must_use]
    pub fn manufacturer(&self) -> Option<&str> {
        self.manufacturer.as_deref()
    }

    /// Product name of the device, if the operating system provides it.
    #[must_use]
    pub fn product(&self) -> Option<&str> {
        self.product.as_deref()
    }

    /// Identifier of the bus the device is attached to (on Linux the bus
    /// number, as in `lsusb`).
    #[must_use]
    pub fn bus_id(&self) -> &str {
        &self.bus_id
    }

    /// Address of the device on its bus (on Linux the device number, as in
    /// `lsusb`). It changes every time the device is plugged in.
    #[must_use]
    pub fn device_address(&self) -> u8 {
        self.device_address
    }
}

/// Picks the device to open among `candidates`, all Quantis USB devices.
///
/// * `serial == None`: exactly one device must be attached; with more than
///   one the choice is refused instead of guessed.
/// * `serial == Some(s)`: exactly one device must have serial number `s`
///   (exact, case-sensitive match).
///
/// The original library selects a device by its index in the enumeration
/// order instead. That order is not stable (it can change when devices are
/// plugged in or out), so a serial number is used here. The original
/// selection also has an off-by-one bug (`QuantisUsbOpen` in
/// `QuantisUsb_Unix.c`):
///
/// ```c
/// if ((quantisUsbCount == 0) || (quantisUsbCount < deviceHandle->deviceNumber))
/// ```
///
/// accepts `deviceNumber == quantisUsbCount`, one past the last device, and
/// then reads an uninitialized slot of the `malloc`ed device array. There is
/// no index here, so that cannot happen.
///
/// # Errors
///
/// * [`Error::NoDevice`] if `candidates` is empty.
/// * [`Error::TooManyDevices`] if the choice is ambiguous: more than one
///   candidate without `serial`, or more than one with serial `s`.
/// * [`Error::SerialNotFound`] if no candidate has serial `s`.
pub(crate) fn select_device<T>(
    candidates: Vec<T>,
    serial: Option<&str>,
    serial_of: impl Fn(&T) -> Option<&str>,
) -> Result<T, Error> {
    if candidates.is_empty() {
        return Err(Error::NoDevice);
    }

    let mut matching: Vec<T> = match serial {
        None => candidates,
        Some(s) => candidates
            .into_iter()
            .filter(|c| serial_of(c) == Some(s))
            .collect(),
    };

    match (matching.len(), serial) {
        (1, _) => Ok(matching.remove(0)),
        (0, Some(s)) => Err(Error::SerialNotFound(s.to_string())),
        // `0` without a serial is excluded by the emptiness check above
        _ => Err(Error::TooManyDevices),
    }
}

#[cfg(test)]
mod tests {
    use super::select_device;
    use crate::error::Error;

    fn select(candidates: &[Option<&'static str>], serial: Option<&str>) -> Result<usize, Error> {
        let indexed: Vec<(usize, Option<&str>)> = candidates.iter().copied().enumerate().collect();
        select_device(indexed, serial, |(_, s)| *s).map(|(i, _)| i)
    }

    #[test]
    fn test_select_without_serial() {
        assert!(matches!(select(&[], None), Err(Error::NoDevice)));
        assert_eq!(select(&[Some("A")], None).unwrap(), 0);
        assert_eq!(select(&[None], None).unwrap(), 0);
        assert!(matches!(
            select(&[Some("A"), Some("B")], None),
            Err(Error::TooManyDevices)
        ));
    }

    #[test]
    fn test_select_by_serial() {
        let attached = [Some("A"), None, Some("B")];
        assert_eq!(select(&attached, Some("A")).unwrap(), 0);
        assert_eq!(select(&attached, Some("B")).unwrap(), 2);

        // exact match only
        assert!(matches!(
            select(&attached, Some("a")),
            Err(Error::SerialNotFound(s)) if s == "a"
        ));
        assert!(matches!(
            select(&attached, Some("")),
            Err(Error::SerialNotFound(_))
        ));

        // a single device must still match the requested serial
        assert!(matches!(
            select(&[Some("A")], Some("B")),
            Err(Error::SerialNotFound(_))
        ));
        assert!(matches!(
            select(&[None], Some("A")),
            Err(Error::SerialNotFound(_))
        ));
    }

    #[test]
    fn test_select_by_serial_no_device() {
        assert!(matches!(select(&[], Some("A")), Err(Error::NoDevice)));
    }

    #[test]
    fn test_select_duplicate_serial_is_refused() {
        assert!(matches!(
            select(&[Some("A"), Some("A")], Some("A")),
            Err(Error::TooManyDevices)
        ));
    }
}
