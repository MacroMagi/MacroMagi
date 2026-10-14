//! Screen-space geometry. All coordinates are physical pixels on the virtual
//! desktop, with the origin at the top-left of the primary monitor.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
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
        Point::new(self.x + (self.w / 2) as i32, self.y + (self.h / 2) as i32)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_clips_to_overlap() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 60, 100, 100);
        assert_eq!(a.intersect(&b), Some(Rect::new(50, 60, 50, 40)));
        assert_eq!(a.intersect(&Rect::new(200, 0, 10, 10)), None);
    }
}
