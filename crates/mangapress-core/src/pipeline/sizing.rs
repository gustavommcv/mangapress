use super::{fill_is_black, spread, OutputFormat, PipelineOptions};
use crate::crop::Background;
use crate::resize::{self, ResizeOptions};

/// Fits a page to the device — or, for an upright whole spread, applies
/// that case's own rule. The same for a grayscale page and a color one.
pub(super) fn resize_for_device<P: image::Pixel<Subpixel = u8> + 'static>(
    page: image::ImageBuffer<P, Vec<u8>>,
    role: spread::PageRole,
    target: (u32, u32),
    options: &PipelineOptions,
    background: Background,
) -> image::ImageBuffer<P, Vec<u8>> {
    if role == spread::PageRole::Rotated && options.no_rotate && !options.wallpaper {
        return resize_upright_spread(page, target);
    }
    resize::resize_page(
        &page,
        &ResizeOptions {
            target,
            upscale: options.upscale,
            stretch: options.stretch,
            wallpaper: options.wallpaper,
            // A custom resolution is KCC's Custom profile, not KDX.
            is_kdx_profile: options.profile.code == "KDX"
                && options.width_override.is_none()
                && options.height_override.is_none(),
            pads_for_cbz_or_pdf: matches!(
                options.output_format,
                OutputFormat::Cbz | OutputFormat::Pdf
            ),
            white_borders: options.white_borders,
            fill: if fill_is_black(options, background) {
                0
            } else {
                255
            },
        },
    )
}

/// How a whole spread that was not to be rotated is sized: left alone
/// unless it is larger than two device widths by one device height, and
/// then only shrunk to fit that. Never fitted to the device like a page,
/// never enlarged.
///
/// This is upstream's rule for every device but a Kindle. For a Kindle
/// profile's EPUB upstream caps the spread at 1920x1920 instead — the limit
/// it observes for Amazon's converter, a tenth narrower than the two
/// screens of a Kindle 11 — and sends a Kindle Scribe's through the
/// ordinary page resize. Neither serves a book read in KOReader, so
/// mangapress applies the one rule to every device.
pub(super) fn resize_upright_spread<P: image::Pixel<Subpixel = u8> + 'static>(
    page: image::ImageBuffer<P, Vec<u8>>,
    target: (u32, u32),
) -> image::ImageBuffer<P, Vec<u8>> {
    let (w, h) = page.dimensions();
    if w > target.0 * 2 || h > target.1 {
        resize::contain(
            &page,
            (target.0 * 2, target.1),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        page
    }
}
