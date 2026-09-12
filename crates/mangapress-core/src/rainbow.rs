//! FFT-based "rainbow artifact" removal for color e-ink (Kaleido-style
//! panels), enabled via `--eraserainbow`.
//!
//! Port target: `erase_rainbow_artifacts()` / `attenuate_diagonal_frequencies()`
//! in KCC's `rainbow_artifacts_eraser.py` (no license header upstream —
//! treat as spec, see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! Technique: 2D real FFT (`rustfft`) of the luminance channel, attenuate
//! frequency components within a diagonal angular band (`target_angle =
//! 135°`, tolerance TBD from source) at radial frequency >= `0.30`
//! cycles/pixel by a factor of `0.10`, then inverse-FFT back. For color
//! images this operates on the Y channel of an RGB<->YUV conversion and the
//! result is reassembled with the original chroma; for grayscale it runs
//! directly on the L-mode image. Runs *after* resize in the pipeline.

use image::GrayImage;

pub fn erase_rainbow_artifacts_gray(_page: &GrayImage) -> GrayImage {
    todo!("port erase_rainbow_artifacts (grayscale path) using rustfft — needs real-photo-ish fixtures to validate")
}
