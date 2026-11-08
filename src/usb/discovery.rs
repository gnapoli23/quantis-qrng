//! List and select the attached Quantis USB devices.

use crate::error::Error;

/// A Quantis USB device attached to the system.
///
/// It is returned by [`QuantisUsb::list`](crate::usb::QuantisUsb::list). The
/// information comes from the operating system: the device is not opened, so
/// it can be in use by another process.
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
    /// Pass it to [`QuantisUsb::open_serial`](crate::usb::QuantisUsb::open_serial)
    /// to open this device.
    #[must_use]
    pub fn serial_number(&self) -> Option<&str> {
        self.serial_number.as_deref()
    }

    /// Manufacturer of the device, if the operating system provides it.
    ///
    /// It is always `None` on Windows.
    #[must_use]
    pub fn manufacturer(&self) -> Option<&str> {
        self.manufacturer.as_deref()
    }

    /// Product name of the device, if the operating system provides it.
    #[must_use]
    pub fn product(&self) -> Option<&str> {
        self.product.as_deref()
    }

    /// Bus of the device (on Linux the bus number shown by `lsusb`).
    #[must_use]
    pub fn bus_id(&self) -> &str {
        &self.bus_id
    }

    /// Address of the device on the bus (on Linux the device number shown by
    /// `lsusb`).
    ///
    /// It changes every time the device is plugged in.
    #[must_use]
    pub fn device_address(&self) -> u8 {
        self.device_address
    }
}

// the only device without `serial`, otherwise the one with that serial.
// selection by serial and not by index -> no off-by-one of the C library
// https://github.com/sanddweller/QRNGPcieDriver/blob/1955b9cce8f0c552c2cee281a9499506100d3170/libs/src/Quantis/QuantisUsb_Unix.c#L439
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
        // 0 without serial is not possible, `candidates` is not empty
        _ => Err(Error::TooManyDevices),
    }
}

#[cfg(test)]
mod tests {
    use super::select_device;
    use crate::error::Error;

    // the candidates are the serial numbers, `select_device` returns the chosen one

    #[test]
    fn test_select_without_serial() {
        assert!(matches!(
            select_device(Vec::<Option<&str>>::new(), None, |s| *s),
            Err(Error::NoDevice)
        ));
        assert_eq!(
            select_device(vec![Some("A")], None, |s| *s).unwrap(),
            Some("A")
        );
        assert_eq!(select_device(vec![None], None, |s| *s).unwrap(), None);
        assert!(matches!(
            select_device(vec![Some("A"), Some("B")], None, |s| *s),
            Err(Error::TooManyDevices)
        ));
    }

    #[test]
    fn test_select_by_serial() {
        let attached = vec![Some("A"), None, Some("B")];
        assert_eq!(
            select_device(attached.clone(), Some("A"), |s| *s).unwrap(),
            Some("A")
        );
        assert_eq!(
            select_device(attached.clone(), Some("B"), |s| *s).unwrap(),
            Some("B")
        );

        // exact match only
        assert!(matches!(
            select_device(attached.clone(), Some("a"), |s| *s),
            Err(Error::SerialNotFound(s)) if s == "a"
        ));
        assert!(matches!(
            select_device(attached, Some(""), |s| *s),
            Err(Error::SerialNotFound(_))
        ));

        // a single device must still match the requested serial
        assert!(matches!(
            select_device(vec![Some("A")], Some("B"), |s| *s),
            Err(Error::SerialNotFound(_))
        ));
        assert!(matches!(
            select_device(vec![None], Some("A"), |s| *s),
            Err(Error::SerialNotFound(_))
        ));
    }

    #[test]
    fn test_select_by_serial_no_device() {
        assert!(matches!(
            select_device(Vec::<Option<&str>>::new(), Some("A"), |s| *s),
            Err(Error::NoDevice)
        ));
    }

    #[test]
    fn test_select_duplicate_serial_is_refused() {
        assert!(matches!(
            select_device(vec![Some("A"), Some("A")], Some("A"), |s| *s),
            Err(Error::TooManyDevices)
        ));
    }
}
