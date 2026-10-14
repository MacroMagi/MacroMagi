//! Observing the user's real input, system-wide.
//!
//! Each platform module exposes one function, `run`, which blocks its thread
//! forever and calls a sink for every keyboard and mouse event.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::run;

#[cfg(not(windows))]
mod rdev;
#[cfg(not(windows))]
pub(crate) use self::rdev::run;
