//! The boundary between a macro and the real machine.
//!
//! Engines never talk to the operating system directly. Everything a macro can
//! *do* (send input) or *sense* (pointer position, screen pixels) goes through
//! a [`Host`]. The desktop app supplies one backed by the OS; tests and dry
//! runs use [`SimHost`], which only pretends.

use std::sync::Mutex;

use thiserror::Error;

use crate::geom::{Point, Rect};
use crate::input::InputEvent;
use crate::nodes::window::{ScreenKind, WindowMatch};

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub struct HostError(pub String);

impl HostError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// A captured piece of the screen: tightly packed RGBA8 rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Screen position of the frame's top-left pixel.
    pub origin: Point,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    pub fn new(origin: Point, width: u32, height: u32, rgba: Vec<u8>) -> Self {
        assert_eq!(rgba.len(), width as usize * height as usize * 4, "frame buffer has the wrong size");
        Self { origin, width, height, rgba }
    }

    /// A frame filled with one color.
    pub fn solid(origin: Point, width: u32, height: u32, rgb: [u8; 3]) -> Self {
        let pixel = [rgb[0], rgb[1], rgb[2], 255];
        Self::new(origin, width, height, pixel.repeat(width as usize * height as usize))
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.origin.x, self.origin.y, self.width, self.height)
    }

    /// Copies the part of the frame covered by `region` (in screen coordinates).
    pub fn crop(&self, region: Rect) -> Option<Frame> {
        let area = self.rect().intersect(&region)?;
        let (x0, y0) = ((area.x - self.origin.x) as usize, (area.y - self.origin.y) as usize);
        let (w, h) = (area.w as usize, area.h as usize);
        let stride = self.width as usize * 4;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in y0..y0 + h {
            let start = row * stride + x0 * 4;
            rgba.extend_from_slice(&self.rgba[start..start + w * 4]);
        }
        Some(Frame { origin: Point::new(area.x, area.y), width: area.w, height: area.h, rgba })
    }

    /// Overwrites part of this frame with another one. `at` is in screen coordinates.
    pub fn blit(&mut self, other: &Frame, at: Point) {
        for row in 0..other.height as i32 {
            for col in 0..other.width as i32 {
                let (x, y) = (at.x + col - self.origin.x, at.y + row - self.origin.y);
                if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                    continue;
                }
                let src = (row as usize * other.width as usize + col as usize) * 4;
                let dst = (y as usize * self.width as usize + x as usize) * 4;
                self.rgba[dst..dst + 4].copy_from_slice(&other.rgba[src..src + 4]);
            }
        }
    }
}

/// Which window to look for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowQuery {
    pub text: String,
    pub by: WindowMatch,
}

/// A window that was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub title: String,
    /// Executable name, e.g. `game.exe`.
    pub process: String,
    /// How the window occupies the screen, as detected. Never `Smart`.
    pub kind: ScreenKind,
    /// Whether it is the window receiving input right now.
    pub foreground: bool,
}

/// A notification for the person at the computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub message: String,
    /// Play a short sound.
    pub sound: bool,
    /// Show a desktop notification.
    pub popup: bool,
}

/// Where sound is listened to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoundSource {
    /// What the computer is playing (speakers or headphones).
    #[default]
    Output,
    /// The microphone.
    Input,
}

/// A stretch of sound: mono samples in `-1..=1`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundClip {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    /// How many samples the source has produced in all, up to the last one
    /// here. Lets a listener skip what it has already seen.
    pub end: u64,
}

pub trait Host: Send + Sync {
    /// Sends one input event to the system.
    fn send(&self, event: &InputEvent) -> Result<(), HostError>;

    /// Where the pointer is right now.
    fn cursor(&self) -> Result<Point, HostError>;

    /// Captures a region of the screen, or the whole primary screen for `None`.
    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError>;

    /// The rectangles of all monitors, primary first.
    fn screens(&self) -> Result<Vec<Rect>, HostError> {
        Ok(vec![self.capture(None)?.rect()])
    }

    /// The executable name of the program whose window has keyboard focus,
    /// e.g. `game.exe`. `None` where this cannot be determined.
    fn foreground_process(&self) -> Option<String> {
        None
    }

    /// Finds a window and brings it to the front, restoring it if minimised.
    /// `Ok(None)` if no window matches.
    fn focus_window(&self, query: &WindowQuery) -> Result<Option<WindowInfo>, HostError> {
        let _ = query;
        Err(HostError::new("switching windows is not supported on this system yet"))
    }

    /// The text on the clipboard; empty when it holds no text.
    fn clipboard_text(&self) -> Result<String, HostError> {
        Err(HostError::new("the clipboard is not available here"))
    }

    /// Plays a sound and/or shows a desktop notification.
    fn notify(&self, notification: &Notification) -> Result<(), HostError> {
        let _ = notification;
        Err(HostError::new("notifications are not available here"))
    }

    /// The last `ms` milliseconds of sound from `source`. The first call may
    /// return less while listening starts.
    fn listen(&self, source: SoundSource, device: &str, ms: f64) -> Result<SoundClip, HostError> {
        let _ = (source, device, ms);
        Err(HostError::new("listening to sound is not available here"))
    }

    /// The words in a picture (optical character recognition), with where
    /// each is in the picture. `language` is a tag such as `en-US`, or empty
    /// for the system's languages.
    fn read_text(&self, picture: &Frame, language: &str) -> Result<Vec<TextWord>, HostError> {
        let _ = (picture, language);
        Err(HostError::new("reading text from the screen is not available on this system yet"))
    }
}

/// A word read from a picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextWord {
    pub text: String,
    /// Where it is, in the picture's pixels.
    pub rect: Rect,
    /// The line it is on, counting from 0.
    pub line: usize,
}

/// A host that sends nothing. It keeps a virtual pointer and a log of
/// everything it was asked to send, and can show a fixed picture as "the screen".
#[derive(Debug, Default)]
pub struct SimHost {
    state: Mutex<SimState>,
}

#[derive(Debug, Default)]
struct SimState {
    cursor: Point,
    sent: Vec<InputEvent>,
    screen: Option<Frame>,
    foreground: Option<String>,
    windows: Vec<WindowInfo>,
    clipboard: String,
    notifications: Vec<Notification>,
    sound: SoundClip,
    screens: Option<Vec<Rect>>,
    words: Vec<TextWord>,
}

impl SimHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `capture` return (parts of) this frame.
    pub fn with_screen(screen: Frame) -> Self {
        let host = Self::default();
        host.set_screen(Some(screen));
        host
    }

    pub fn set_screen(&self, screen: Option<Frame>) {
        self.lock().screen = screen;
    }

    /// Pretends this program has keyboard focus.
    pub fn set_foreground(&self, process: Option<&str>) {
        self.lock().foreground = process.map(str::to_owned);
    }

    /// Pretends these windows exist, for Switch to Window.
    pub fn set_windows(&self, windows: Vec<WindowInfo>) {
        self.lock().windows = windows;
    }

    /// Pretends these displays exist (instead of one, the size of the screen picture).
    pub fn set_screens(&self, screens: Vec<Rect>) {
        self.lock().screens = Some(screens);
    }

    pub fn set_clipboard(&self, text: &str) {
        self.lock().clipboard = text.to_owned();
    }

    /// Notifications shown so far.
    pub fn notifications(&self) -> Vec<Notification> {
        self.lock().notifications.clone()
    }

    /// Makes `listen` return this sound.
    pub fn set_sound(&self, clip: SoundClip) {
        self.lock().sound = clip;
    }

    /// Makes `read_text` find these words.
    pub fn set_text(&self, words: Vec<TextWord>) {
        self.lock().words = words;
    }

    /// Everything sent so far.
    pub fn sent(&self) -> Vec<InputEvent> {
        self.lock().sent.clone()
    }

    /// Removes and returns everything sent so far.
    pub fn take_sent(&self) -> Vec<InputEvent> {
        std::mem::take(&mut self.lock().sent)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SimState> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Host for SimHost {
    fn send(&self, event: &InputEvent) -> Result<(), HostError> {
        let mut state = self.lock();
        match *event {
            InputEvent::MoveTo { x, y } => state.cursor = Point::new(x, y),
            InputEvent::MoveBy { dx, dy } => {
                state.cursor = Point::new(state.cursor.x + dx, state.cursor.y + dy);
            }
            _ => {}
        }
        state.sent.push(event.clone());
        Ok(())
    }

    fn cursor(&self) -> Result<Point, HostError> {
        Ok(self.lock().cursor)
    }

    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError> {
        let state = self.lock();
        let screen = state.screen.as_ref().ok_or_else(|| HostError::new("no screen is available here"))?;
        match region {
            None => Ok(screen.clone()),
            Some(region) => screen.crop(region).ok_or_else(|| HostError::new("the search area is outside the screen")),
        }
    }

    fn foreground_process(&self) -> Option<String> {
        self.lock().foreground.clone()
    }

    fn focus_window(&self, query: &WindowQuery) -> Result<Option<WindowInfo>, HostError> {
        let mut state = self.lock();
        let needle = query.text.to_lowercase();
        let found = state.windows.iter().find(|w| match query.by {
            WindowMatch::Title => w.title.to_lowercase().contains(&needle),
            WindowMatch::Process => w.process.to_lowercase().contains(&needle),
        });
        let Some(found) = found.cloned() else { return Ok(None) };
        state.foreground = Some(found.process.clone());
        Ok(Some(WindowInfo { foreground: true, ..found }))
    }

    fn screens(&self) -> Result<Vec<Rect>, HostError> {
        let screens = self.lock().screens.clone();
        match screens {
            Some(screens) => Ok(screens),
            None => Ok(vec![self.capture(None)?.rect()]),
        }
    }

    fn clipboard_text(&self) -> Result<String, HostError> {
        Ok(self.lock().clipboard.clone())
    }

    fn notify(&self, notification: &Notification) -> Result<(), HostError> {
        self.lock().notifications.push(notification.clone());
        Ok(())
    }

    fn read_text(&self, _picture: &Frame, _language: &str) -> Result<Vec<TextWord>, HostError> {
        Ok(self.lock().words.clone())
    }

    fn listen(&self, _source: SoundSource, _device: &str, ms: f64) -> Result<SoundClip, HostError> {
        let state = self.lock();
        let clip = &state.sound;
        let wanted = ((ms / 1000.0) * f64::from(clip.sample_rate)) as usize;
        let start = clip.samples.len().saturating_sub(wanted);
        Ok(SoundClip {
            sample_rate: clip.sample_rate,
            samples: clip.samples[start..].to_vec(),
            end: clip.samples.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_and_blit_agree_on_coordinates() {
        let mut screen = Frame::solid(Point::new(0, 0), 20, 10, [0, 0, 0]);
        let patch = Frame::solid(Point::new(0, 0), 3, 2, [255, 0, 0]);
        screen.blit(&patch, Point::new(5, 4));

        let cropped = screen.crop(Rect::new(5, 4, 3, 2)).unwrap();
        assert_eq!(cropped.origin, Point::new(5, 4));
        assert_eq!(cropped.rgba, patch.rgba);

        // Cropping past the edge clips rather than failing.
        let edge = screen.crop(Rect::new(18, 8, 10, 10)).unwrap();
        assert_eq!((edge.width, edge.height), (2, 2));
    }

    #[test]
    fn sim_host_tracks_the_pointer() {
        let host = SimHost::new();
        host.send(&InputEvent::MoveTo { x: 10, y: 20 }).unwrap();
        host.send(&InputEvent::MoveBy { dx: 5, dy: -5 }).unwrap();
        assert_eq!(host.cursor().unwrap(), Point::new(15, 15));
        assert_eq!(host.take_sent().len(), 2);
        assert!(host.sent().is_empty());
    }
}
