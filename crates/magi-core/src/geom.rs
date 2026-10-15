//! Screen-space geometry, in physical pixels.
//!
//! A position or area is either **global** – on the virtual desktop, with the
//! origin at the top-left of the primary display – or **on a display**:
//! relative to the top-left of one display, numbered in [`Displays`] order.
//! Positions people pick are stored on their display, so a macro keeps
//! working when displays are rearranged; positions a macro computes (where an
//! image was found) are global.

use serde::{Deserialize, Serialize};

fn is_none<T>(value: &Option<T>) -> bool {
    value.is_none()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
    /// The display `x, y` are relative to (0 = primary). `None`: global.
    #[serde(default, skip_serializing_if = "is_none")]
    pub display: Option<u8>,
}

impl Point {
    /// A global position.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y, display: None }
    }

    /// A position relative to a display.
    pub const fn on(display: u8, x: i32, y: i32) -> Self {
        Self { x, y, display: Some(display) }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// The display `x, y` are relative to (0 = primary). `None`: global.
    #[serde(default, skip_serializing_if = "is_none")]
    pub display: Option<u8>,
}

impl Rect {
    /// A global area.
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h, display: None }
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add_unsigned(self.w)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add_unsigned(self.h)
    }

    pub fn center(&self) -> Point {
        Point { x: self.x + (self.w / 2) as i32, y: self.y + (self.h / 2) as i32, display: self.display }
    }

    pub fn origin(&self) -> Point {
        Point { x: self.x, y: self.y, display: self.display }
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    /// The overlapping area of two rectangles, if any.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        (right > x && bottom > y).then(|| Rect::new(x, y, (right - x) as u32, (bottom - y) as u32))
    }
}

/// The displays, in a stable order: the primary display (the one at the
/// origin) first, then the others from left to right, top to bottom.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Displays(Vec<Rect>);

impl Displays {
    /// `rects` are the displays' global areas, in any order.
    pub fn new(mut rects: Vec<Rect>) -> Self {
        rects.retain(|r| !r.is_empty());
        for rect in &mut rects {
            rect.display = None;
        }
        rects.sort_by_key(|r| (!r.contains(Point::new(0, 0)), r.x, r.y));
        Self(rects)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// A display's global area.
    pub fn get(&self, index: u8) -> Option<Rect> {
        self.0.get(usize::from(index)).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, Rect)> + '_ {
        self.0.iter().enumerate().map(|(i, r)| (i as u8, *r))
    }

    /// The display a global position is on, or the nearest one.
    pub fn index_of(&self, p: Point) -> Option<u8> {
        if let Some(i) = self.0.iter().position(|r| r.contains(p)) {
            return Some(i as u8);
        }
        let distance = |r: &Rect| {
            let dx = (r.x - p.x).max(p.x - (r.right() - 1)).max(0) as i64;
            let dy = (r.y - p.y).max(p.y - (r.bottom() - 1)).max(0) as i64;
            dx * dx + dy * dy
        };
        self.0.iter().enumerate().min_by_key(|(_, r)| distance(r)).map(|(i, _)| i as u8)
    }

    /// Where the origin of a display is. A display that is not connected
    /// falls back to the primary one, so the position still lands somewhere.
    fn origin(&self, display: u8) -> (i32, i32) {
        self.get(display).or_else(|| self.get(0)).map_or((0, 0), |r| (r.x, r.y))
    }

    /// A position as a global one.
    pub fn to_global(&self, p: Point) -> Point {
        match p.display {
            None => p,
            Some(display) => {
                let (ox, oy) = self.origin(display);
                Point::new(ox + p.x, oy + p.y)
            }
        }
    }

    /// A global position, made relative to the display it is on.
    pub fn to_local(&self, p: Point) -> Point {
        let global = self.to_global(p);
        match self.index_of(global) {
            Some(display) => {
                let (ox, oy) = self.origin(display);
                Point::on(display, global.x - ox, global.y - oy)
            }
            None => global,
        }
    }

    pub fn rect_to_global(&self, r: Rect) -> Rect {
        let origin = self.to_global(r.origin());
        Rect::new(origin.x, origin.y, r.w, r.h)
    }

    /// A global area, made relative to the display its top-left corner is on.
    pub fn rect_to_local(&self, r: Rect) -> Rect {
        let origin = self.to_local(r.origin());
        Rect { x: origin.x, y: origin.y, w: r.w, h: r.h, display: origin.display }
    }

    /// The same position on another display: at the same place relative to
    /// its size, so the middle of one display becomes the middle of the other.
    pub fn move_to_display(&self, p: Point, display: u8) -> Point {
        let local = self.to_local(p);
        let from = local.display.and_then(|d| self.get(d));
        let to = self.get(display);
        match (from, to) {
            (Some(from), Some(to)) => {
                let scale = |v: i32, a: u32, b: u32| ((i64::from(v) * i64::from(b)) / i64::from(a.max(1))) as i32;
                Point::on(display, scale(local.x, from.w, to.w), scale(local.y, from.h, to.h))
            }
            _ => Point::on(display, local.x, local.y),
        }
    }

    /// A short name for a display, for menus: `Display 2 (1920 × 1080)`.
    pub fn label(&self, display: u8) -> String {
        match self.get(display) {
            Some(r) if display == 0 => format!("Display 1 – primary ({} × {})", r.w, r.h),
            Some(r) => format!("Display {} ({} × {})", display + 1, r.w, r.h),
            None => format!("Display {} (not connected)", display + 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_displays() -> Displays {
        // A secondary display to the left of the primary one, listed first.
        Displays::new(vec![Rect::new(-1280, 0, 1280, 1024), Rect::new(0, 0, 1920, 1080)])
    }

    #[test]
    fn intersect_clips_to_overlap() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 60, 100, 100);
        assert_eq!(a.intersect(&b), Some(Rect::new(50, 60, 50, 40)));
        assert_eq!(a.intersect(&Rect::new(200, 0, 10, 10)), None);
    }

    #[test]
    fn the_primary_display_comes_first() {
        let displays = two_displays();
        assert_eq!(displays.get(0), Some(Rect::new(0, 0, 1920, 1080)));
        assert_eq!(displays.get(1), Some(Rect::new(-1280, 0, 1280, 1024)));
    }

    #[test]
    fn positions_convert_between_global_and_display_relative() {
        let displays = two_displays();
        let local = displays.to_local(Point::new(-1270, 20));
        assert_eq!(local, Point::on(1, 10, 20));
        assert_eq!(displays.to_global(local), Point::new(-1270, 20));
        assert_eq!(displays.to_global(Point::new(5, 5)), Point::new(5, 5), "global positions stay as they are");
        // A display that is not connected falls back to the primary one.
        assert_eq!(displays.to_global(Point::on(7, 10, 10)), Point::new(10, 10));
        // Old files have no display: their positions are global.
        let old: Point = serde_json::from_str(r#"{"x":3,"y":4}"#).unwrap();
        assert_eq!(old, Point::new(3, 4));
        assert_eq!(serde_json::to_string(&Point::new(3, 4)).unwrap(), r#"{"x":3,"y":4}"#);
    }

    #[test]
    fn moving_to_another_display_keeps_the_relative_place() {
        let displays = two_displays();
        let middle_of_primary = Point::on(0, 960, 540);
        assert_eq!(displays.move_to_display(middle_of_primary, 1), Point::on(1, 640, 512));
        let area = displays.rect_to_local(Rect::new(-1000, 100, 50, 50));
        assert_eq!(area, Rect { x: 280, y: 100, w: 50, h: 50, display: Some(1) });
        assert_eq!(displays.rect_to_global(area), Rect::new(-1000, 100, 50, 50));
    }
}
