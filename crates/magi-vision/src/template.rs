//! Template matching: finding a small image (the *needle*) inside a larger one
//! (the *haystack*).
//!
//! ## The score
//!
//! Every candidate position gets a similarity in `0.0..=1.0`, the product of
//! two things a person means by "it looks the same":
//!
//! * **structure** – the zero-mean normalised cross-correlation of the two
//!   patches over all three color channels. Unrelated content scores near 0,
//!   which is what makes a threshold like 80% meaningful.
//! * **color closeness** – `(1 − RMS pixel difference)²`. Correlation alone
//!   ignores brightness and contrast, so a greyed-out button would otherwise
//!   match its enabled twin perfectly. Squaring it keeps mere "same shape,
//!   different colors" coincidences well below the usual 80% threshold.
//!
//! Identical pixels score exactly `1.0`. A needle with no structure at all (a
//! solid color) is scored on color closeness only.
//!
//! Very small or featureless needles (a bare edge, a 12 px square) can score
//! surprisingly high against unrelated content; the ignored `chance_scores`
//! test prints how high. Distinctive needles of 24 px or more are robust.
//!
//! ## Speed
//!
//! Comparing every position at full resolution is far too slow for a whole
//! screen, so the search is coarse-to-fine: both images are halved up to three
//! times, every position is scored at the coarsest level, and only the best
//! few candidates are followed back up to full resolution.

use rayon::prelude::*;

use crate::ImageRef;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Match {
    /// Top-left corner of the match inside the haystack.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Similarity, `0.0..=1.0`.
    pub score: f32,
}

impl Match {
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

/// Never shrink the needle below this many pixels on its short side: smaller
/// than that and it stops being distinctive.
const MIN_COARSE_SIDE: usize = 8;
const MAX_HALVINGS: usize = 3;
/// Candidates carried from the coarsest level into refinement.
const MAX_CANDIDATES: usize = 16;
/// Candidates kept per haystack row during the coarse scan.
const ROW_CANDIDATES: usize = 3;
/// How far (in pixels of the finer level) a candidate may shift per refinement step.
const REFINE_RADIUS: i64 = 2;
/// Candidates scoring this far below the leader are dropped during refinement.
const PRUNE_MARGIN: f64 = 0.15;
/// Per-sample variance below which a patch counts as a solid color.
const FLAT_VARIANCE: f64 = 1.0;
/// Memory budget for the summed-area tables of one band of the coarse scan.
const BAND_BYTES: usize = 24 * 1024 * 1024;

/// Finds the best match of `needle` in `haystack`.
///
/// Returns the single best position and its score, however low; the caller
/// decides whether that is good enough. `None` only if the needle is empty or
/// does not fit inside the haystack. Alpha is ignored in both images.
pub fn find_template(haystack: ImageRef<'_>, needle: ImageRef<'_>) -> Option<Match> {
    if needle.width == 0 || needle.height == 0 || needle.width > haystack.width || needle.height > haystack.height {
        return None;
    }

    let short_side = needle.width.min(needle.height) as usize;
    let halvings = (0..=MAX_HALVINGS).rev().find(|&d| short_side >> d >= MIN_COARSE_SIDE).unwrap_or(0);

    // levels[0] is full resolution, levels[halvings] the coarsest.
    let mut levels = vec![Level::new(Plane::from_rgba(haystack), Plane::from_rgba(needle))];
    for _ in 0..halvings {
        let finer = levels.last().expect("levels is never empty");
        levels.push(Level::new(finer.haystack.halve(), finer.needle.halve()));
    }

    let mut candidates = levels[halvings].scan();
    for level in levels[..halvings].iter().rev() {
        candidates = level.refine(&candidates);
    }

    let best = candidates.into_iter().max_by(|a, b| a.score.total_cmp(&b.score))?;
    let full = &levels[0];
    let score = if full.is_identical_at(best.x, best.y) { 1.0 } else { best.score.min(0.9999) };
    Some(Match { x: best.x as u32, y: best.y as u32, width: needle.width, height: needle.height, score: score as f32 })
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    x: usize,
    y: usize,
    score: f64,
}

/// An image as interleaved RGB bytes.
struct Plane {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Plane {
    fn from_rgba(image: ImageRef<'_>) -> Self {
        let mut px = Vec::with_capacity(image.rgba.len() / 4 * 3);
        for pixel in image.rgba.as_chunks::<4>().0 {
            px.extend_from_slice(&pixel[..3]);
        }
        Self { w: image.width as usize, h: image.height as usize, px }
    }

    /// Half-size copy, averaging each 2×2 block. Odd trailing rows/columns are dropped.
    fn halve(&self) -> Self {
        let (w, h) = (self.w / 2, self.h / 2);
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            let top = self.span(2 * y, 0, self.w);
            let bottom = self.span(2 * y + 1, 0, self.w);
            for i in (0..w * 6).step_by(6) {
                for c in i..i + 3 {
                    let sum =
                        u16::from(top[c]) + u16::from(top[c + 3]) + u16::from(bottom[c]) + u16::from(bottom[c + 3]);
                    px.push(((sum + 2) / 4) as u8);
                }
            }
        }
        Self { w, h, px }
    }

    /// The bytes of `len` pixels of row `y`, starting at column `x`.
    #[inline]
    fn span(&self, y: usize, x: usize, len: usize) -> &[u8] {
        let start = (y * self.w + x) * 3;
        &self.px[start..start + len * 3]
    }
}

/// Sums over a patch that the score is computed from.
#[derive(Clone, Copy, Default)]
struct Sums {
    channel: [f64; 3],
    squares: f64,
}

impl Sums {
    fn of(plane: &Plane, x: usize, y: usize, w: usize, h: usize) -> Self {
        let (mut channel, mut squares) = ([0u64; 3], 0u64);
        for row in y..y + h {
            for pixel in plane.span(row, x, w).as_chunks::<3>().0 {
                for c in 0..3 {
                    let value = u64::from(pixel[c]);
                    channel[c] += value;
                    squares += value * value;
                }
            }
        }
        Self { channel: channel.map(|v| v as f64), squares: squares as f64 }
    }

    /// Σ over channels of (Σ v² − (Σ v)² / n): the patch's total variance times n.
    fn energy(&self, n: f64) -> f64 {
        self.squares - self.channel.iter().map(|s| s * s).sum::<f64>() / n
    }
}

/// Summed-area tables over a band of rows, for O(1) patch sums.
struct Integral {
    stride: usize,
    first_row: usize,
    channel: [Vec<u32>; 3],
    squares: Vec<u64>,
}

impl Integral {
    /// Covers rows `first_row..end_row` of `plane`.
    ///
    /// The per-channel tables are `u32` and may wrap for large images; patch
    /// sums are taken with wrapping arithmetic and stay exact as long as a
    /// single patch sums to less than 2³², which holds for any needle below
    /// 16 megapixels.
    fn build(plane: &Plane, first_row: usize, end_row: usize) -> Self {
        let stride = plane.w + 1;
        let rows = end_row - first_row + 1;
        let mut channel = [vec![0u32; stride * rows], vec![0u32; stride * rows], vec![0u32; stride * rows]];
        let mut squares = vec![0u64; stride * rows];
        for (r, y) in (first_row..end_row).enumerate() {
            let (above, below) = (r * stride, (r + 1) * stride);
            let (mut run, mut run_squares) = ([0u32; 3], 0u64);
            for (x, pixel) in plane.span(y, 0, plane.w).as_chunks::<3>().0.iter().enumerate() {
                for c in 0..3 {
                    run[c] = run[c].wrapping_add(u32::from(pixel[c]));
                    run_squares += u64::from(pixel[c]) * u64::from(pixel[c]);
                    channel[c][below + x + 1] = channel[c][above + x + 1].wrapping_add(run[c]);
                }
                squares[below + x + 1] = squares[above + x + 1] + run_squares;
            }
        }
        Self { stride, first_row, channel, squares }
    }

    fn sums(&self, x: usize, y: usize, w: usize, h: usize) -> Sums {
        let top = (y - self.first_row) * self.stride;
        let bottom = top + h * self.stride;
        let (a, b, c, d) = (top + x, top + x + w, bottom + x, bottom + x + w);
        let rect = |t: &[u32]| t[d].wrapping_sub(t[b]).wrapping_sub(t[c]).wrapping_add(t[a]);
        Sums {
            channel: [rect(&self.channel[0]), rect(&self.channel[1]), rect(&self.channel[2])].map(f64::from),
            squares: (self.squares[d] + self.squares[a] - self.squares[b] - self.squares[c]) as f64,
        }
    }
}

#[inline]
fn dot(a: &[u8], b: &[u8]) -> u64 {
    // Chunked so the inner sums fit u32 and the loop vectorises.
    a.chunks(4096)
        .zip(b.chunks(4096))
        .map(|(a, b)| u64::from(a.iter().zip(b).map(|(&a, &b)| u32::from(a) * u32::from(b)).sum::<u32>()))
        .sum()
}

/// One resolution of the search.
struct Level {
    haystack: Plane,
    needle: Plane,
    /// Number of samples in the needle (pixels × channels).
    samples: f64,
    needle_sums: Sums,
    needle_energy: f64,
    needle_is_flat: bool,
}

impl Level {
    fn new(haystack: Plane, needle: Plane) -> Self {
        let pixels = (needle.w * needle.h) as f64;
        let needle_sums = Sums::of(&needle, 0, 0, needle.w, needle.h);
        let needle_energy = needle_sums.energy(pixels);
        let samples = pixels * 3.0;
        Self {
            haystack,
            needle,
            samples,
            needle_sums,
            needle_energy,
            needle_is_flat: needle_energy / samples < FLAT_VARIANCE,
        }
    }

    fn positions(&self) -> (usize, usize) {
        (self.haystack.w - self.needle.w + 1, self.haystack.h - self.needle.h + 1)
    }

    /// Σ haystack × needle over the patch at `(x, y)`.
    fn cross(&self, x: usize, y: usize) -> f64 {
        let w = self.needle.w;
        (0..self.needle.h).map(|j| dot(self.haystack.span(y + j, x, w), self.needle.span(j, 0, w))).sum::<u64>() as f64
    }

    /// Similarity of the haystack patch at `(x, y)`, given its sums.
    fn score(&self, x: usize, y: usize, patch: Sums) -> f64 {
        let pixels = self.samples / 3.0;
        // Σ (patch − needle)², needed for the color term either way.
        let squared_error = |cross: f64| (patch.squares - 2.0 * cross + self.needle_sums.squares).max(0.0);
        let closeness = |squared_error: f64| {
            let c = 1.0 - (squared_error / self.samples).sqrt() / 255.0;
            c * c
        };

        if self.needle_is_flat {
            // Σ patch × needle for a solid needle is just mean × Σ patch.
            let cross: f64 = (0..3).map(|c| self.needle_sums.channel[c] / pixels * patch.channel[c]).sum();
            return closeness(squared_error(cross)).clamp(0.0, 1.0);
        }
        let patch_energy = patch.energy(pixels);
        if patch_energy / self.samples < FLAT_VARIANCE {
            return 0.0; // a solid patch cannot look like a structured needle
        }
        let cross = self.cross(x, y);
        let covariance = cross - (0..3).map(|c| patch.channel[c] * self.needle_sums.channel[c]).sum::<f64>() / pixels;
        let correlation = covariance / (patch_energy * self.needle_energy).sqrt();
        (correlation.clamp(0.0, 1.0) * closeness(squared_error(cross))).clamp(0.0, 1.0)
    }

    /// Scores every position and returns the most promising ones.
    fn scan(&self) -> Vec<Candidate> {
        let (columns, rows) = self.positions();
        let separation = (self.needle.w.min(self.needle.h) / 2).max(2);
        let band_rows = (BAND_BYTES / (20 * (self.haystack.w + 1))).max(self.needle.h).max(16);

        let mut found = Vec::new();
        for band_start in (0..rows).step_by(band_rows) {
            let band_end = (band_start + band_rows).min(rows);
            let integral = Integral::build(&self.haystack, band_start, band_end - 1 + self.needle.h);
            let band: Vec<Candidate> = (band_start..band_end)
                .into_par_iter()
                .flat_map_iter(|y| {
                    let scores: Vec<f64> = (0..columns)
                        .map(|x| self.score(x, y, integral.sums(x, y, self.needle.w, self.needle.h)))
                        .collect();
                    best_in_row(&scores, separation).into_iter().map(move |(x, score)| Candidate { x, y, score })
                })
                .collect();
            found.extend(band);
        }
        suppress_neighbours(found, separation, MAX_CANDIDATES)
    }

    /// Re-scores candidates from the next coarser level at this resolution.
    fn refine(&self, coarser: &[Candidate]) -> Vec<Candidate> {
        let (columns, rows) = self.positions();
        let mut refined: Vec<Candidate> = coarser
            .par_iter()
            .map(|candidate| {
                let (cx, cy) = (2 * candidate.x as i64, 2 * candidate.y as i64);
                let mut best = Candidate { x: 0, y: 0, score: -1.0 };
                for y in (cy - REFINE_RADIUS).max(0)..=(cy + REFINE_RADIUS).min(rows as i64 - 1) {
                    for x in (cx - REFINE_RADIUS).max(0)..=(cx + REFINE_RADIUS).min(columns as i64 - 1) {
                        let (x, y) = (x as usize, y as usize);
                        let patch = Sums::of(&self.haystack, x, y, self.needle.w, self.needle.h);
                        let score = self.score(x, y, patch);
                        if score > best.score {
                            best = Candidate { x, y, score };
                        }
                    }
                }
                best
            })
            .filter(|candidate| candidate.score >= 0.0)
            .collect();

        refined.sort_by(|a, b| b.score.total_cmp(&a.score));
        if let Some(leader) = refined.first().map(|c| c.score) {
            refined.retain(|c| c.score >= leader - PRUNE_MARGIN);
            refined.truncate((refined.len() / 2).max(2));
        }
        refined
    }

    fn is_identical_at(&self, x: usize, y: usize) -> bool {
        let w = self.needle.w;
        (0..self.needle.h).all(|j| self.haystack.span(y + j, x, w) == self.needle.span(j, 0, w))
    }
}

/// The best few positions of one row, at least `separation` columns apart.
fn best_in_row(scores: &[f64], separation: usize) -> Vec<(usize, f64)> {
    let mut best: Vec<(usize, f64)> = Vec::with_capacity(ROW_CANDIDATES);
    for (x, &score) in scores.iter().enumerate() {
        if score <= 0.0 {
            continue;
        }
        if let Some(near) = best.iter_mut().find(|(bx, _)| bx.abs_diff(x) < separation) {
            if score > near.1 {
                *near = (x, score);
            }
        } else if best.len() < ROW_CANDIDATES {
            best.push((x, score));
        } else if let Some(weakest) = best.iter_mut().min_by(|a, b| a.1.total_cmp(&b.1))
            && score > weakest.1
        {
            *weakest = (x, score);
        }
    }
    best
}

/// Keeps the strongest candidates, dropping any within `separation` of a stronger one.
fn suppress_neighbours(mut candidates: Vec<Candidate>, separation: usize, limit: usize) -> Vec<Candidate> {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Candidate> = Vec::with_capacity(limit);
    for candidate in candidates {
        if kept.len() == limit {
            break;
        }
        let crowded =
            kept.iter().any(|k| k.x.abs_diff(candidate.x) < separation && k.y.abs_diff(candidate.y) < separation);
        if !crowded {
            kept.push(candidate);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bitmap;

    /// Small deterministic generator so the tests need no extra dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as u32
        }

        fn below(&mut self, n: u32) -> u32 {
            self.next() % n
        }
    }

    fn fill(bitmap: &mut Bitmap, x: u32, y: u32, w: u32, h: u32, rgb: [u8; 3]) {
        for row in y..(y + h).min(bitmap.height) {
            for col in x..(x + w).min(bitmap.width) {
                let i = ((row * bitmap.width + col) * 4) as usize;
                bitmap.rgba[i..i + 3].copy_from_slice(&rgb);
            }
        }
    }

    /// Something that looks like a desktop: overlapping panels, small details, a little noise.
    fn fake_screen(width: u32, height: u32, seed: u64) -> Bitmap {
        let mut rng = Rng(seed);
        let mut screen = Bitmap { width, height, rgba: [40, 44, 52, 255].repeat((width * height) as usize) };
        for _ in 0..(width * height / 900) {
            let (w, h) = (4 + rng.below(90), 4 + rng.below(50));
            let (x, y) = (rng.below(width), rng.below(height));
            let rgb = [rng.below(256) as u8, rng.below(256) as u8, rng.below(256) as u8];
            fill(&mut screen, x, y, w, h, rgb);
        }
        for pixel in screen.rgba.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(rng.below(5) as u8);
            }
        }
        screen
    }

    fn crop(source: &Bitmap, x: u32, y: u32, w: u32, h: u32) -> Bitmap {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in y..y + h {
            let start = ((row * source.width + x) * 4) as usize;
            rgba.extend_from_slice(&source.rgba[start..start + (w * 4) as usize]);
        }
        Bitmap { width: w, height: h, rgba }
    }

    #[test]
    fn finds_exact_crops_at_awkward_offsets_and_sizes() {
        let screen = fake_screen(640, 360, 1);
        // Covers no halving, one, two and three halvings, at odd positions.
        for (x, y, w, h) in [(13, 7, 11, 9), (101, 53, 27, 19), (333, 201, 61, 45), (417, 143, 150, 97)] {
            let needle = crop(&screen, x, y, w, h);
            let found = find_template(screen.as_ref(), needle.as_ref()).unwrap();
            assert_eq!((found.x, found.y), (x, y), "needle {w}×{h}");
            assert_eq!(found.score, 1.0, "needle {w}×{h}");
            assert_eq!(found.center(), (x + w / 2, y + h / 2));
        }
    }

    #[test]
    fn tolerates_noise_and_reports_a_high_score() {
        let screen = fake_screen(640, 360, 2);
        let mut needle = crop(&screen, 250, 120, 48, 40);
        let mut rng = Rng(99);
        for pixel in needle.rgba.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                *channel = (i32::from(*channel) + rng.below(13) as i32 - 6).clamp(0, 255) as u8;
            }
        }
        let found = find_template(screen.as_ref(), needle.as_ref()).unwrap();
        assert_eq!((found.x, found.y), (250, 120));
        assert!(found.score > 0.9 && found.score < 1.0, "score {}", found.score);
    }

    #[test]
    fn something_that_is_not_there_scores_low() {
        let screen = fake_screen(640, 360, 3);
        let elsewhere = fake_screen(200, 200, 4);
        let needle = crop(&elsewhere, 60, 60, 48, 40);
        let found = find_template(screen.as_ref(), needle.as_ref()).unwrap();
        assert!(found.score < 0.6, "score {}", found.score);
    }

    #[test]
    fn color_matters_not_only_shape() {
        let button = |rgb| {
            let mut b = Bitmap { width: 40, height: 24, rgba: [128, 128, 128, 255].repeat(40 * 24) };
            fill(&mut b, 6, 5, 28, 14, rgb);
            b
        };
        let mut screen = Bitmap { width: 300, height: 200, rgba: [128, 128, 128, 255].repeat(300 * 200) };
        fill(&mut screen, 106, 85, 28, 14, [40, 200, 60]); // a green button at (100, 80)

        let green = find_template(screen.as_ref(), button([40, 200, 60]).as_ref()).unwrap();
        assert_eq!((green.x, green.y, green.score), (100, 80, 1.0));
        let red = find_template(screen.as_ref(), button([220, 50, 40]).as_ref()).unwrap();
        assert!(red.score < 0.5, "a red button must not match a green one (score {})", red.score);
    }

    #[test]
    fn a_dimmed_copy_is_similar_but_not_identical() {
        let screen = fake_screen(320, 200, 5);
        let mut needle = crop(&screen, 90, 60, 48, 40);
        for pixel in needle.rgba.as_chunks_mut::<4>().0 {
            for channel in &mut pixel[..3] {
                *channel = (f32::from(*channel) * 0.5 + 60.0) as u8; // "disabled" look
            }
        }
        let found = find_template(screen.as_ref(), needle.as_ref()).unwrap();
        assert_eq!((found.x, found.y), (90, 60));
        assert!(found.score < 0.9, "a washed-out copy should lose points (score {})", found.score);
    }

    #[test]
    fn solid_needles_are_matched_on_color() {
        let mut screen = Bitmap { width: 120, height: 80, rgba: [20, 20, 20, 255].repeat(120 * 80) };
        fill(&mut screen, 70, 30, 20, 20, [255, 60, 0]);
        let orange = Bitmap { width: 10, height: 10, rgba: [255, 60, 0, 255].repeat(100) };
        let found = find_template(screen.as_ref(), orange.as_ref()).unwrap();
        assert_eq!(found.score, 1.0);
        assert!((70..=80).contains(&found.x) && (30..=40).contains(&found.y));

        let blue = Bitmap { width: 10, height: 10, rgba: [0, 60, 255, 255].repeat(100) };
        assert!(find_template(screen.as_ref(), blue.as_ref()).unwrap().score < 0.7);
    }

    #[test]
    fn a_needle_that_does_not_fit_is_not_found() {
        let screen = fake_screen(50, 50, 6);
        let needle = fake_screen(60, 10, 7);
        assert_eq!(find_template(screen.as_ref(), needle.as_ref()), None);
    }

    #[test]
    fn the_needle_may_be_the_whole_haystack() {
        let screen = fake_screen(64, 48, 8);
        let found = find_template(screen.as_ref(), screen.as_ref()).unwrap();
        assert_eq!((found.x, found.y, found.score), (0, 0, 1.0));
    }

    #[test]
    #[ignore = "calibration report"]
    fn chance_scores() {
        for (w, h) in [(16, 16), (32, 24), (48, 40), (96, 64)] {
            let mut worst: f32 = 0.0;
            let mut total = 0.0;
            for seed in 0..12u64 {
                let screen = fake_screen(960, 540, 100 + seed);
                let elsewhere = fake_screen(300, 300, 500 + seed);
                let needle = crop(&elsewhere, 60, 60, w, h);
                let s = find_template(screen.as_ref(), needle.as_ref()).unwrap().score;
                worst = worst.max(s);
                total += s;
            }
            println!("absent {w}x{h}: mean {:.3} max {:.3}", total / 12.0, worst);
        }
    }

    /// Run with `cargo test -p magi-vision --release -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing report, not a correctness test"]
    fn timing_on_a_full_hd_screen() {
        let screen = fake_screen(1920, 1080, 9);
        for (w, h) in [(12, 12), (24, 24), (48, 48), (96, 64), (300, 200)] {
            let needle = crop(&screen, 1500, 800, w, h);
            let start = std::time::Instant::now();
            let found = find_template(screen.as_ref(), needle.as_ref()).unwrap();
            println!("{w:>4}×{h:<4} {:>7.1?}  at ({}, {})  score {}", start.elapsed(), found.x, found.y, found.score);
        }
    }
}
