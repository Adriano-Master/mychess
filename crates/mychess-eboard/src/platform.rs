//! The only platform-specific code of the crate (spec m8-10, D2703).

/// The port name offered first on this platform: Scid's default on Linux,
/// the usual name of a USB-serial adapter elsewhere.
pub fn default_port() -> &'static str {
    #[cfg(windows)]
    {
        "COM3"
    }
    #[cfg(target_os = "macos")]
    {
        "/dev/cu.usbserial"
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        "/dev/ttyUSB0"
    }
}

/// Windows process creation flag: no console window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows: do not open a console window for the driver.
#[cfg(windows)]
pub(crate) fn hide_console(command: &mut tokio::process::Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Other platforms have no console window to hide.
#[cfg(not(windows))]
pub(crate) fn hide_console(_command: &mut tokio::process::Command) {}
