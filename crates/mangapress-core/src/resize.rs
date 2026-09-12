//! Resize filter selection and fit modes.
//!
//! Port target: `resize_method()` and `resizeImage()` in KCC's `image.py`
//! (GPLv3-licensed upstream — reimplement from this spec, do not copy; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! Filter rule (`resize_method`): if the source image already fits within
//! the target resolution on both axes, use bicubic; otherwise (the common
//! case — manga scans are almost always higher-res than e-ink screens) use
//! Lanczos3. KCC does **not** upscale small images unless `-u/--upscale` is
//! passed — that early-return needs to be preserved exactly, not "fixed."
//!
//! Fit mode is chosen by comparing the source aspect ratio to the target's
//! within [`ASPECT_MATCH_TOLERANCE`] (KCC's `AUTO_CROP_THRESHOLD = 0.015`,
//! tripled for the `KDX` profile specifically):
//! - within tolerance: crop-to-fill (`ImageOps.fit` equivalent)
//! - `--stretch`: plain resize, ignoring aspect ratio
//! - otherwise: fit-within + pad with the page's detected background color
//!   (`ImageOps.pad` / `ImageOps.contain` equivalent)

use image::imageops::FilterType;

/// KCC's `AUTO_CROP_THRESHOLD`. The `KDX` profile uses `3.0 *` this value.
pub const ASPECT_MATCH_TOLERANCE: f64 = 0.015;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// Aspect ratio close enough to the device's — crop to fill exactly.
    Fill,
    /// Fit within target bounds, pad the remainder with the background color.
    Pad,
    /// `--stretch`: ignore aspect ratio entirely.
    Stretch,
}

/// Picks Lanczos3 for downscaling, bicubic for same-size/upscale — matching
/// `resize_method()`'s filter choice (not necessarily its early-return
/// no-upscale behavior, which belongs to the caller).
pub fn choose_filter(src: (u32, u32), target: (u32, u32)) -> FilterType {
    let fits = src.0 <= target.0 && src.1 <= target.1;
    if fits {
        FilterType::CatmullRom
    } else {
        FilterType::Lanczos3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscale_uses_lanczos() {
        assert_eq!(
            choose_filter((2000, 3000), (1072, 1448)),
            FilterType::Lanczos3
        );
    }

    #[test]
    fn same_size_uses_bicubic_equivalent() {
        assert_eq!(
            choose_filter((1072, 1448), (1072, 1448)),
            FilterType::CatmullRom
        );
    }
}
