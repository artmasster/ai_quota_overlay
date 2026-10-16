//! Platform layers: native Win32 on Windows; winit plus own presenting on macOS and Linux (X11).
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{run, UNINSTALLER};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use self::unix::run;

#[cfg(not(windows))]
pub const UNINSTALLER: &str = "";