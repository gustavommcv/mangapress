//! FFT-based "rainbow artifact" removal for color e-ink (Kaleido-style
//! panels), enabled via `--eraserainbow`. Runs *after* resize in the
//! pipeline (matches upstream's `optimizeForDisplay()` call site).
//!
//! Port target: `erase_rainbow_artifacts()` / `attenuate_diagonal_frequencies()`
//! in KCC's `rainbow_artifacts_eraser.py` (no license header upstream —
//! treated with the same caution as GPLv3 files per
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`). Only the grayscale path
//! is relevant here (this pipeline has no color support yet) — upstream's
//! color path additionally converts to YUV and only filters the Y channel,
//! reassembling afterward; the filtering math itself is identical.
//!
//! Technique: 2D FFT of the page, attenuating (multiplying by
//! [`ATTENUATION_FACTOR`]) any frequency bin whose radial frequency is at
//! least [`FREQ_THRESHOLD`] cycles/pixel *and* whose angle falls within
//! [`ANGLE_TOLERANCE`] degrees of one of the four diagonal directions (45,
//! 135, 225, 315 — upstream's `target_angle=135` plus its complement and
//! both perpendiculars), then inverting back to the spatial domain. This
//! targets Moire interference between the manga's own halftone screentone
//! pattern and an e-ink color-filter array's diagonal subpixel grid.
//!
//! One confirmed discrepancy in upstream itself, not introduced here:
//! `attenuate_diagonal_frequencies()`'s docstring claims `angle_tolerance`
//! defaults to 15 degrees, but the actual function signature default is
//! 10 — the *signature* is what the code that runs actually uses, so
//! [`ANGLE_TOLERANCE`] is 10, not 15.
//!
//! Uses a full complex 2D FFT (`rustfft`, row passes then column passes)
//! rather than `numpy.fft.rfft2`'s real-input half-spectrum optimization —
//! mathematically equivalent for this purpose: attenuating a real signal's
//! full spectrum by the same real-valued factor at every bin (including
//! the negative-frequency mirror rfft2 omits) preserves the Hermitian
//! symmetry a real image's spectrum has, so the inverse transform is still
//! real (up to float noise, discarded by taking the real part). This
//! avoids pulling in a separate real-FFT crate for one algorithm.

use image::{GrayImage, Luma};
use rustfft::num_complex::Complex32;
use rustfft::FftPlanner;
use std::cell::RefCell;

const FREQ_THRESHOLD: f64 = 0.30;
const TARGET_ANGLE_DEG: f64 = 135.0;
const ANGLE_TOLERANCE_DEG: f64 = 10.0;
const ATTENUATION_FACTOR: f32 = 0.10;

thread_local! {
    // `FftPlanner` caches its computed algorithms per transform length --
    // reusing one across pages (nearly all of which share the same
    // post-resize dimensions) avoids replanning the same row/column FFTs on
    // every single page.
    static PLANNER: RefCell<FftPlanner<f32>> = RefCell::new(FftPlanner::new());
}

pub fn erase_rainbow_artifacts_gray(page: &GrayImage) -> GrayImage {
    let (w, h) = page.dimensions();
    if w <= 1 || h <= 1 {
        // Matches optimizeForDisplay()'s `all(dim > 1 for dim in size)` guard.
        return page.clone();
    }

    let mut spectrum: Vec<Complex32> = page
        .pixels()
        .map(|p| Complex32::new(p[0] as f32, 0.0))
        .collect();

    PLANNER.with(|planner| {
        let mut planner = planner.borrow_mut();
        fft_2d(&mut spectrum, w as usize, h as usize, &mut planner, false);
        attenuate_diagonal_frequencies(&mut spectrum, w as usize, h as usize);
        fft_2d(&mut spectrum, w as usize, h as usize, &mut planner, true);
    });

    let scale = 1.0 / (w as f32 * h as f32);

    GrayImage::from_fn(w, h, |x, y| {
        let idx = (y as usize) * (w as usize) + (x as usize);
        let value = (spectrum[idx].re * scale).clamp(0.0, 255.0);
        Luma([value as u8])
    })
}

/// In-place 2D FFT (or inverse, unnormalized like `rustfft` always is) via
/// separable row-then-column 1D transforms.
fn fft_2d(
    data: &mut [Complex32],
    w: usize,
    h: usize,
    planner: &mut FftPlanner<f32>,
    inverse: bool,
) {
    let row_fft = if inverse {
        planner.plan_fft_inverse(w)
    } else {
        planner.plan_fft_forward(w)
    };
    for row in data.chunks_mut(w) {
        row_fft.process(row);
    }

    let col_fft = if inverse {
        planner.plan_fft_inverse(h)
    } else {
        planner.plan_fft_forward(h)
    };
    let mut column = vec![Complex32::new(0.0, 0.0); h];
    for x in 0..w {
        for (y, slot) in column.iter_mut().enumerate() {
            *slot = data[y * w + x];
        }
        col_fft.process(&mut column);
        for (y, &value) in column.iter().enumerate() {
            data[y * w + x] = value;
        }
    }
}

/// `attenuate_diagonal_frequencies()`, operating on the full spectrum
/// in-place (see module docs for why this is equivalent to upstream's
/// half-spectrum `rfft2` approach).
fn attenuate_diagonal_frequencies(spectrum: &mut [Complex32], w: usize, h: usize) {
    let freq_x = fftfreq(w);
    let freq_y = fftfreq(h);
    let freq_threshold_sq = FREQ_THRESHOLD * FREQ_THRESHOLD;
    let targets = [
        TARGET_ANGLE_DEG,
        (TARGET_ANGLE_DEG + 180.0) % 360.0,
        (TARGET_ANGLE_DEG + 90.0) % 360.0,
        (TARGET_ANGLE_DEG + 270.0) % 360.0,
    ];

    for y in 0..h {
        for x in 0..w {
            let fx = freq_x[x];
            let fy = freq_y[y];
            if fx * fx + fy * fy < freq_threshold_sq {
                continue;
            }
            let angle_deg = fy.atan2(fx).to_degrees().rem_euclid(360.0);
            if targets
                .iter()
                .any(|&target| angle_matches(angle_deg, target, ANGLE_TOLERANCE_DEG))
            {
                spectrum[y * w + x] *= ATTENUATION_FACTOR;
            }
        }
    }
}

fn angle_matches(angle_deg: f64, target_deg: f64, tolerance_deg: f64) -> bool {
    let min_angle = (target_deg - tolerance_deg).rem_euclid(360.0);
    let max_angle = (target_deg + tolerance_deg).rem_euclid(360.0);
    if min_angle > max_angle {
        angle_deg >= min_angle || angle_deg <= max_angle
    } else {
        (min_angle..=max_angle).contains(&angle_deg)
    }
}

/// `numpy.fft.fftfreq(n, d=1.0)`: bin `i` maps to `i/n` for
/// `i <= (n-1)/2`, else `(i-n)/n` — correct for both even and odd `n`
/// (verified against both cases, not just the common even case).
fn fftfreq(n: usize) -> Vec<f64> {
    let n_f = n as f64;
    (0..n)
        .map(|i| {
            let i_f = i as f64;
            if i <= (n - 1) / 2 {
                i_f / n_f
            } else {
                (i_f - n_f) / n_f
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fftfreq_matches_numpy_for_even_n() {
        // np.fft.fftfreq(8) == [0, .125, .25, .375, -.5, -.375, -.25, -.125]
        let f = fftfreq(8);
        let expected = [0.0, 0.125, 0.25, 0.375, -0.5, -0.375, -0.25, -0.125];
        for (a, b) in f.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn fftfreq_matches_numpy_for_odd_n() {
        // np.fft.fftfreq(7) == [0, 1/7, 2/7, 3/7, -3/7, -2/7, -1/7]
        let f = fftfreq(7);
        let expected = [
            0.0,
            1.0 / 7.0,
            2.0 / 7.0,
            3.0 / 7.0,
            -3.0 / 7.0,
            -2.0 / 7.0,
            -1.0 / 7.0,
        ];
        for (a, b) in f.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-12, "{a} vs {b}");
        }
    }

    #[test]
    fn angle_matches_within_tolerance_no_wraparound() {
        assert!(angle_matches(135.0, 135.0, 10.0));
        assert!(angle_matches(127.0, 135.0, 10.0));
        assert!(angle_matches(144.0, 135.0, 10.0));
        assert!(!angle_matches(120.0, 135.0, 10.0));
        assert!(!angle_matches(150.0, 135.0, 10.0));
    }

    #[test]
    fn angle_matches_handles_wraparound_at_zero() {
        // target=0 with tolerance=10 should match both 355 and 5.
        assert!(angle_matches(355.0, 0.0, 10.0));
        assert!(angle_matches(5.0, 0.0, 10.0));
        assert!(!angle_matches(180.0, 0.0, 10.0));
    }

    #[test]
    fn tiny_dimension_is_left_untouched() {
        let img = GrayImage::from_pixel(1, 50, Luma([100]));
        let out = erase_rainbow_artifacts_gray(&img);
        assert_eq!(out, img);
    }

    #[test]
    fn flat_image_is_unchanged() {
        // A perfectly flat image has all its energy in the DC bin (freq 0),
        // which never meets the frequency threshold -- nothing should be
        // attenuated, and round-tripping through FFT/IFFT should reproduce
        // the same flat value (allowing for float rounding).
        let img = GrayImage::from_pixel(16, 16, Luma([128]));
        let out = erase_rainbow_artifacts_gray(&img);
        for p in out.pixels() {
            assert!((p[0] as i32 - 128).abs() <= 1, "pixel={}", p[0]);
        }
    }

    #[test]
    fn diagonal_high_frequency_pattern_is_attenuated_towards_flat() {
        // A checkerboard-like diagonal stripe pattern concentrates energy
        // exactly in the targeted band (high radial frequency, ~45 degree
        // diagonal orientation). After filtering, the result should have
        // much less variance than the original.
        let n = 64u32;
        let img = GrayImage::from_fn(n, n, |x, y| Luma([if (x + y) % 2 == 0 { 0 } else { 255 }]));
        let out = erase_rainbow_artifacts_gray(&img);

        let variance = |im: &GrayImage| -> f64 {
            let values: Vec<f64> = im.pixels().map(|p| p[0] as f64).collect();
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64
        };

        assert!(
            variance(&out) < variance(&img) * 0.5,
            "expected filtering to significantly reduce diagonal high-frequency variance: before={}, after={}",
            variance(&img),
            variance(&out)
        );
    }
}
