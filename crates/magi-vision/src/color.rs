use crate::ImageRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorMatch {
    /// The first matching pixel, scanning rows top to bottom.
    pub x: u32,
    pub y: u32,
    /// How many pixels matched in total.
    pub count: u32,
}

/// Finds pixels whose red, green and blue are each within `tolerance` of `rgb`.
pub fn find_color(image: ImageRef<'_>, rgb: [u8; 3], tolerance: u8) -> Option<ColorMatch> {
    let width = image.width as usize;
    let mut first = None;
    let mut count = 0u32;
    for (index, pixel) in image.rgba.as_chunks::<4>().0.iter().enumerate() {
        let near = pixel[..3].iter().zip(rgb).all(|(&channel, want)| channel.abs_diff(want) <= tolerance);
        if near {
            count += 1;
            first.get_or_insert(index);
        }
    }
    first.map(|index| ColorMatch { x: (index % width) as u32, y: (index / width) as u32, count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_first_pixel_within_tolerance() {
        let mut rgba = [10u8, 10, 10, 255].repeat(5 * 4);
        let at = |x: usize, y: usize| (y * 5 + x) * 4;
        rgba[at(3, 1)..at(3, 1) + 3].copy_from_slice(&[250, 5, 0]);
        rgba[at(1, 2)..at(1, 2) + 3].copy_from_slice(&[255, 0, 0]);
        let image = ImageRef::new(5, 4, &rgba);

        assert_eq!(find_color(image, [255, 0, 0], 0), Some(ColorMatch { x: 1, y: 2, count: 1 }));
        assert_eq!(find_color(image, [255, 0, 0], 8), Some(ColorMatch { x: 3, y: 1, count: 2 }));
        assert_eq!(find_color(image, [0, 255, 0], 8), None);
    }
}
