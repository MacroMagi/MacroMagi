//! The displays and the microphones, as the editor shows them: displays
//! numbered the way macros store them (see [`Displays`]), both read from the
//! system now and then rather than every frame.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use magi_core::geom::{Displays, Rect};

/// How long the list is reused before it is read again.
const REFRESH: Duration = Duration::from_secs(3);

struct Cache {
    read: Instant,
    /// The monitors in the system's own order.
    system: Vec<Rect>,
    displays: Displays,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<R>(f: impl FnOnce(&Cache) -> R) -> R {
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if cache.as_ref().is_none_or(|c| c.read.elapsed() > REFRESH) {
        let system = magi_platform::capture::system_order().unwrap_or_default();
        let displays = Displays::new(system.clone());
        *cache = Some(Cache { read: Instant::now(), system, displays });
    }
    f(cache.as_ref().expect("filled above"))
}

/// The connected displays. Empty if they cannot be read.
pub fn current() -> Displays {
    with_cache(|c| c.displays.clone())
}

/// The names of the microphones and other sound inputs.
pub fn microphones() -> Vec<String> {
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if cache.as_ref().is_none_or(|(read, _)| read.elapsed() > REFRESH) {
        *cache = Some((Instant::now(), magi_platform::audio::input_devices()));
    }
    cache.as_ref().map(|(_, names)| names.clone()).unwrap_or_default()
}

/// The number the windowing system gives a display, for opening a window full
/// screen on it.
pub fn monitor_index(display: u8) -> Option<usize> {
    with_cache(|c| {
        let rect = c.displays.get(display)?;
        c.system.iter().position(|r| r.x == rect.x && r.y == rect.y && r.w == rect.w && r.h == rect.h)
    })
}
