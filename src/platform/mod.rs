//! Platform layers: native Win32 on Windows; winit plus own presenting on macOS and Linux (X11).
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{run, UNINSTALLER};

#[cfg(not(windows))]
pub const UNINSTALLER: &str = "";