//! Finding and talking to Steam Controllers over HID.

use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice, HidResult};

use crate::protocol::{self, Report};

/// How often to resend the settings report. The controller restores lizard mode after ~3 s.
const SETTINGS_REFRESH: Duration = Duration::from_secs(2);

/// Vendor-defined HID usage page that carries the controller's raw reports.
const VENDOR_USAGE_PAGE_MIN: u16 = 0xFF00;

/// Whether a HID interface is one the controller sends its raw state on.
pub fn is_controller_interface(info: &DeviceInfo) -> bool {
    if info.vendor_id() != protocol::VALVE_VID {
        return false;
    }
    let pid_ok = match info.product_id() {
        protocol::PID_PROTEUS_DONGLE | protocol::PID_NEREID_DONGLE => (2..=5).contains(&info.interface_number()),
        protocol::PID_TRITON_WIRED | protocol::PID_TRITON_BLE => true,
        _ => false,
    };
    // On Windows each interface is split into collections (mouse, keyboard, vendor);
    // only the vendor collection carries controller reports. Other platforms report 0 here.
    pid_ok && (info.usage_page() == 0 || info.usage_page() >= VENDOR_USAGE_PAGE_MIN)
}

/// An hidapi context that only enumerates Valve devices. Enumerating every HID device on the
/// system can block indefinitely on Windows when some unrelated device stops responding.
pub fn valve_api() -> HidResult<HidApi> {
    HidApi::disable_device_discovery();
    let mut api = HidApi::new()?;
    api.add_devices(protocol::VALVE_VID, 0)?;
    Ok(api)
}

/// Re-enumerate Valve devices only (see [`valve_api`]).
pub fn rescan(api: &mut HidApi) -> HidResult<()> {
    api.reset_devices()?;
    api.add_devices(protocol::VALVE_VID, 0)
}

/// Every Valve HID entry, for debugging.
pub fn list_valve_devices(api: &HidApi) -> Vec<&DeviceInfo> {
    api.device_list().filter(|d| d.vendor_id() == protocol::VALVE_VID).collect()
}

pub fn find_controller_interfaces(api: &HidApi) -> Vec<&DeviceInfo> {
    api.device_list().filter(|d| is_controller_interface(d)).collect()
}

pub struct Controller {
    dev: HidDevice,
    last_settings: Option<Instant>,
}

impl Controller {
    pub fn open(api: &HidApi, info: &DeviceInfo) -> HidResult<Self> {
        Ok(Self { dev: info.open_device(api)?, last_settings: None })
    }

    /// Turn off lizard mode and enable raw IMU data. Safe to call often; it rate-limits itself.
    pub fn keep_configured(&mut self) -> HidResult<()> {
        if self.last_settings.is_some_and(|t| t.elapsed() < SETTINGS_REFRESH) {
            return Ok(());
        }
        self.last_settings = Some(Instant::now());
        self.dev.send_feature_report(&protocol::settings_report())
    }

    /// Ask the controller to power off (see [`protocol::turn_off_report`]).
    pub fn turn_off(&self) -> HidResult<()> {
        self.dev.send_feature_report(&protocol::turn_off_report())
    }

    /// Read one raw report into `buf`; returns the number of bytes, 0 on timeout.
    pub fn read_raw(&self, buf: &mut [u8], timeout_ms: i32) -> HidResult<usize> {
        self.dev.read_timeout(buf, timeout_ms)
    }

    /// Read and parse one report, or `None` on timeout.
    pub fn read(&mut self, timeout_ms: i32) -> HidResult<Option<Report>> {
        self.keep_configured()?;
        let mut buf = [0u8; 64];
        let n = self.read_raw(&mut buf, timeout_ms)?;
        Ok(if n == 0 { None } else { protocol::parse(&buf[..n]) })
    }
}
