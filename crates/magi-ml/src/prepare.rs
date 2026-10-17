//! Turning a picture of the screen into the numbers a model reads.

use magi_core::magic::{ColorOrder, InputSpec, Layout};

/// The cubic convolution kernel (a = −0.5), the "bicubic" of most image tools.
fn cubic(x: f64) -> f64 {
    let a = -0.5;
    let x = x.abs();
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

/// For each output position along one axis, the input positions it reads and
/// their weights. When shrinking, the kernel is widened by the shrink factor,
/// so every input pixel contributes (no aliasing).
fn weights(input: usize, output: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = input as f64 / output as f64;
    let support = 2.0 * scale.max(1.0);
    let filter_scale = scale.max(1.0);
    (0..output)
        .map(|o| {
            let center = (o as f64 + 0.5) * scale;
            let first = ((center - support).floor().max(0.0)) as usize;
            let last = ((center + support).ceil() as usize).min(input);
            let mut taps: Vec<f64> = (first..last).map(|i| cubic((i as f64 + 0.5 - center) / filter_scale)).collect();
            let total: f64 = taps.iter().sum();
            if total.abs() > f64::EPSILON {
                taps.iter_mut().for_each(|w| *w /= total);
            }
            (first, taps.into_iter().map(|w| w as f32).collect())
        })
        .collect()
}

/// Resizes tightly packed RGBA pixels to `out_w × out_h` RGB values (0–255,
/// as floats), with an antialiased bicubic filter.
pub fn resize_rgb(rgba: &[u8], width: u32, height: u32, out_w: u32, out_h: u32) -> Vec<f32> {
    let (w, h, ow, oh) = (width as usize, height as usize, out_w as usize, out_h as usize);
    // Across first: h rows of ow pixels.
    let across = weights(w, ow);
    let mut middle = vec![0.0f32; h * ow * 3];
    for y in 0..h {
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        for (x, (first, taps)) in across.iter().enumerate() {
            let mut sum = [0.0f32; 3];
            for (k, weight) in taps.iter().enumerate() {
                let i = (first + k) * 4;
                for c in 0..3 {
                    sum[c] += f32::from(row[i + c]) * weight;
                }
            }
            middle[(y * ow + x) * 3..(y * ow + x) * 3 + 3].copy_from_slice(&sum);
        }
    }
    // Then down.
    let down = weights(h, oh);
    let mut out = vec![0.0f32; oh * ow * 3];
    for (y, (first, taps)) in down.iter().enumerate() {
        for x in 0..ow {
            let mut sum = [0.0f32; 3];
            for (k, weight) in taps.iter().enumerate() {
                let i = ((first + k) * ow + x) * 3;
                for c in 0..3 {
                    sum[c] += middle[i + c] * weight;
                }
            }
            for c in 0..3 {
                // Like an 8-bit picture: rounded and kept in range.
                out[(y * ow + x) * 3 + c] = sum[c].round().clamp(0.0, 255.0);
            }
        }
    }
    out
}

/// The model's input for a picture: resized, scaled, normalised and laid out
/// as the spec says.
pub fn prepare(rgba: &[u8], width: u32, height: u32, spec: &InputSpec) -> Vec<f32> {
    let (ow, oh) = (spec.width.max(1), spec.height.max(1));
    let rgb = if (width, height) == (ow, oh) {
        rgba.as_chunks::<4>().0.iter().flat_map(|p| [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])]).collect()
    } else {
        resize_rgb(rgba, width, height, ow, oh)
    };
    let pixels = (ow * oh) as usize;
    let mut out = vec![0.0f32; pixels * 3];
    for p in 0..pixels {
        for c in 0..3 {
            let source = match spec.color {
                ColorOrder::Rgb => c,
                ColorOrder::Bgr => 2 - c,
            };
            let value = (rgb[p * 3 + source] * spec.scale - spec.mean[c]) / spec.std[c].max(1e-6);
            let index = match spec.layout {
                Layout::Nchw => c * pixels + p,
                Layout::Nhwc => p * 3 + c,
            };
            out[index] = value;
        }
    }
    out
}

/// Turns raw scores into probabilities that add up to one.
pub fn softmax(scores: &[f32]) -> Vec<f32> {
    let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exp: Vec<f32> = scores.iter().map(|s| (s - max).exp()).collect();
    let total: f32 = exp.iter().sum();
    exp.into_iter().map(|e| e / total.max(f32::MIN_POSITIVE)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_color_stays_that_color_at_any_size() {
        let rgba: Vec<u8> = (0..40 * 30).flat_map(|_| [200u8, 100, 50, 255]).collect();
        for (w, h) in [(10, 10), (80, 60), (40, 30)] {
            let out = resize_rgb(&rgba, 40, 30, w, h);
            assert!(out.chunks(3).all(|p| p == [200.0, 100.0, 50.0]), "{w}×{h}");
        }
    }

    #[test]
    fn inputs_are_normalised_and_laid_out_as_asked() {
        let rgba = [255u8, 0, 0, 255, 0, 0, 255, 255];
        let mut spec = InputSpec { width: 2, height: 1, mean: [0.0; 3], std: [1.0; 3], ..InputSpec::default() };
        assert_eq!(prepare(&rgba, 2, 1, &spec), [1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        spec.layout = Layout::Nhwc;
        assert_eq!(prepare(&rgba, 2, 1, &spec), [1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        spec.color = ColorOrder::Bgr;
        assert_eq!(prepare(&rgba, 2, 1, &spec), [0.0, 0.0, 1.0, 1.0, 0.0, 0.0]);
        let sum: f32 = softmax(&[1.0, 2.0, 3.0]).iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }
}
