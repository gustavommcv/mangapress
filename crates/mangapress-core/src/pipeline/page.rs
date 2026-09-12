//! Single output page: one `ComicPage` in KCC terms. A source image may
//! produce one, two (split spread), or three (split + rotated) of these.
//!
//! Per-page pipeline order, from `imgFileProcessing()`/`ComicPage` in
//! upstream `comic2ebook.py`/`image.py` — order matters, don't reshuffle
//! without re-checking the source:
//! 1. Crop: page-number-aware margin (default), plain margin, or disabled
//!    (mutually exclusive) — [`crate::crop::page_number`] /
//!    [`crate::crop::margin`].
//! 2. Inter-panel crop, if enabled — [`crate::crop::inter_panel`].
//! 3. Gamma correction — [`crate::contrast::gamma_correct`].
//! 4. Grayscale conversion (unless `--forcecolor`).
//! 5. Autolevel (optional, `--autolevel`) then autocontrast — [`crate::contrast`].
//! 6. Resize to device resolution — [`crate::resize`].
//! 7. Rainbow-artifact removal, if enabled — runs *after* resize —
//!    [`crate::rainbow`].
//! 8. Palette quantization (`--forcepng`/`--force-png-rgb` only) —
//!    [`crate::quantize`].
//! 9. Encode (JPEG by default, PNG if quantized/forced, WEBP if
//!    `--webp`) and, for oversized Kindle Scribe pages, split top/bottom
//!    into `-above`/`-below` halves.

pub struct ComicPage {
    pub image: image::GrayImage,
}
