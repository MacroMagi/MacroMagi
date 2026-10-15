//! Reading pixels from the screen.

use magi_core::geom::{Point, Rect};
use magi_core::host::{Frame, HostError};
use xcap::Monitor;

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    pub name: String,
    /// Position and size on the virtual desktop, in physical pixels.
    pub rect: Rect,
    pub scale: f32,
    pub primary: bool,
}

fn error(e: xcap::XCapError) -> HostError {
    HostError::new(format!(
        "could not capture the screen: {e}. On macOS, grant MacroMagi the Screen Recording permission."
    ))
}

fn describe(monitor: &Monitor) -> Result<MonitorInfo, HostError> {
    Ok(MonitorInfo {
        name: monitor.name().unwrap_or_default(),
        rect: Rect::new(
            monitor.x().map_err(error)?,
            monitor.y().map_err(error)?,
            monitor.width().map_err(error)?,
            monitor.height().map_err(error)?,
        ),
        scale: monitor.scale_factor().unwrap_or(1.0),
        primary: monitor.is_primary().unwrap_or(false),
    })
}

/// Every connected monitor, primary first.
pub fn monitors() -> Result<Vec<MonitorInfo>, HostError> {
    let mut list = Monitor::all().map_err(error)?.iter().map(describe).collect::<Result<Vec<_>, _>>()?;
    list.sort_by_key(|m| !m.primary);
    Ok(list)
}

/// Every monitor's area, in the order the operating system lists them. This
/// is the order windowing libraries number monitors in (winit's
/// `available_monitors`), which a window needs to go full screen on one.
pub fn system_order() -> Result<Vec<Rect>, HostError> {
    Monitor::all().map_err(error)?.iter().map(|m| describe(m).map(|info| info.rect)).collect()
}

/// Captures a region of the virtual desktop, or the whole primary monitor for `None`.
///
/// A region is captured from the monitor it overlaps most and clipped to it;
/// regions spanning several monitors are not stitched together yet.
pub fn capture(region: Option<Rect>) -> Result<Frame, HostError> {
    let all = Monitor::all().map_err(error)?;
    let mut best: Option<(&Monitor, MonitorInfo, Rect)> = None;
    for monitor in &all {
        let info = describe(monitor)?;
        let overlap = match region {
            Some(region) => region.intersect(&info.rect),
            None => info.primary.then_some(info.rect),
        };
        if let Some(overlap) = overlap {
            let area = |r: &Rect| u64::from(r.w) * u64::from(r.h);
            if best.as_ref().is_none_or(|(_, _, current)| area(&overlap) > area(current)) {
                best = Some((monitor, info, overlap));
            }
        }
    }
    // No monitor reported itself as primary: fall back to the first one.
    if best.is_none()
        && region.is_none()
        && let Some(monitor) = all.first()
    {
        let info = describe(monitor)?;
        let rect = info.rect;
        best = Some((monitor, info, rect));
    }
    let (monitor, info, area) = best.ok_or_else(|| match region {
        Some(_) => HostError::new("the search area is not on any screen"),
        None => HostError::new("no screen was found"),
    })?;

    let image = if area == info.rect {
        monitor.capture_image()
    } else {
        monitor.capture_region((area.x - info.rect.x) as u32, (area.y - info.rect.y) as u32, area.w, area.h)
    }
    .map_err(error)?;
    Ok(Frame::new(Point::new(area.x, area.y), image.width(), image.height(), image.into_raw()))
}
