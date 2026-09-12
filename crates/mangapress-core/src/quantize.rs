//! Grayscale palette quantization with dithering (`--forcepng`/`--force-png-rgb`
//! output path only — the default JPEG path ships full 8-bit tone).
//!
//! Port target: `quantizeImage()` in `image.py`. Upstream calls Pillow's
//! `Image.quantize()` without an explicit `dither=` argument, which defaults
//! to Floyd-Steinberg error diffusion — so quantization is dithered
//! implicitly, not by an explicit KCC choice. Since every profile here is
//! grayscale-only (see [`crate::profile::Palette`]), this only needs to
//! quantize the L channel to N evenly-spaced gray levels, not general RGB
//! quantization — no need for a full color-quantization crate.

use crate::profile::Palette;
use image::GrayImage;

pub fn quantize_with_floyd_steinberg(_page: &GrayImage, _palette: Palette) -> GrayImage {
    todo!("implement N-level gray quantization + Floyd-Steinberg dithering, validate against synthetic gradient fixtures")
}
