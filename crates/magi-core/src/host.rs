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

    /// A frame filled with one colour.
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

pub trait Host: Send + Sync {
    /// Sends one input event to the system.
    fn send(&self, event: &InputEvent) -> Result<(), HostError>;

    /// Where the pointer is right now.
    fn cursor(&self) -> Result<Point, HostError>;

    /// Captures a region of the screen, or the whole primary screen for `None`.
    fn capture(&self, region: Option<Rect>) -> Result<Frame, HostError>;
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
