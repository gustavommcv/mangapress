use super::{
    fill_is_black, sizing::resize_for_device, spread, OutputFormat, PipelineOptions, ProcessedPage,
};
use crate::crop::Background;
use crate::error::Result;
use crate::quantize::Container;

pub(super) fn finish_page(
    page: image::RgbImage,
    role: spread::PageRole,
    target: (u32, u32),
    options: &PipelineOptions,
    background: Background,
    is_color: bool,
) -> Result<ProcessedPage> {
    let effective_gamma = options
        .gamma
        .filter(|&g| g >= 0.1)
        .unwrap_or(options.profile.gamma);
    let page = crate::contrast::gamma_correct_rgb(&page, effective_gamma);
    if is_color && options.force_color {
        return finish_color_page(page, role, target, options, background);
    }
    let page = crate::color::to_gray(&page);

    let page =
        if options.noautocontrast || options.webtoon || (is_color && !options.color_autocontrast) {
            page
        } else {
            crate::contrast::autocontrast(&page, options.autolevel)
        };

    let page = resize_for_device(page, role, target, options, background);

    let page = if options.erase_rainbow {
        crate::rainbow::erase_rainbow_artifacts_gray(&page)
    } else {
        page
    };

    let bytes;
    let extension = if options.force_png {
        let palette = options.palette();
        match quantized_container(options) {
            Container::IndexedPng => {
                let indices = crate::quantize::quantize_to_palette_indices(&page, palette);
                bytes = crate::quantize::encode_indexed_png(page.dimensions(), &indices, palette)?;
                "png"
            }
            Container::GrayPng => {
                let gray = if options.no_quantize {
                    page.clone()
                } else {
                    crate::quantize::quantize_with_floyd_steinberg(&page, palette)
                };
                bytes = crate::png_out::encode(
                    gray.as_raw(),
                    gray.width(),
                    gray.height(),
                    image::ExtendedColorType::L8,
                )?;
                "png"
            }
        }
    } else {
        // Not `DynamicImage::write_to(..., ImageFormat::Jpeg)`: that always
        // encodes at the `image` crate's own default quality of 75,
        // regardless of device profile -- noticeably more compressed than
        // KCC's own 85/90 default for every page this pipeline produces.
        let quality = options.jpeg_quality();
        bytes = crate::jpeg::encode(
            page.as_raw(),
            page.width(),
            page.height(),
            crate::jpeg::Samples::Gray,
            quality,
        )?;
        "jpg"
    };
    Ok(ProcessedPage {
        extension: extension.to_string(),
        bytes,
        black_background: fill_is_black(options, background),
        role,
        source_truncated: false,
    })
}

/// How a `--forcepng` page is stored — see [`Container`]. Upstream's rule
/// minus its GIF branch: plain grayscale where upstream turns the page back
/// into grayscale (or never makes it a palette image), a palette PNG
/// otherwise. The oldest-Kindle exception does not survive a custom
/// resolution, as upstream's doesn't (it renames the profile "Custom" before
/// it checks).
pub(super) fn quantized_container(options: &PipelineOptions) -> Container {
    let custom_resolution =
        options.width_override.unwrap_or(0) != 0 || options.height_override.unwrap_or(0) != 0;
    let oldest_kindle =
        !custom_resolution && matches!(options.profile.code, "K1" | "K2" | "K34" | "KDX");

    if options.no_quantize
        || options.png_legacy
        || options.output_format == OutputFormat::Pdf
        || (options.output_format == OutputFormat::Cbz && oldest_kindle)
    {
        Container::GrayPng
    } else {
        Container::IndexedPng
    }
}

/// The rest of the pipeline for a color page kept in color
/// (`--forcecolor`), after gamma: no grayscale conversion and no
/// quantization; autocontrast only with `--colorautocontrast`, and then on
/// all three channels at once so the colors keep their balance; resized as
/// a gray page is; saved as RGB JPEG, or as PNG with `--force-png-rgb`.
///
/// With `--force-png-rgb` the page is saved as RGB PNG on every device;
/// upstream writes a 256-color GIF for a Kindle profile's EPUB, which this
/// crate does not do for grayscale pages either (see
/// [`crate::quantize`]).
fn finish_color_page(
    page: image::RgbImage,
    role: spread::PageRole,
    target: (u32, u32),
    options: &PipelineOptions,
    background: Background,
) -> Result<ProcessedPage> {
    let page = if !options.noautocontrast && !options.webtoon && options.color_autocontrast {
        crate::contrast::autocontrast_rgb(&page, options.autolevel)
    } else {
        page
    };
    let page = resize_for_device(page, role, target, options, background);
    let page = if options.erase_rainbow {
        crate::rainbow::erase_rainbow_artifacts_rgb(&page)
    } else {
        page
    };

    let bytes;
    let extension = if options.force_png && options.force_png_rgb {
        bytes = crate::png_out::encode(
            page.as_raw(),
            page.width(),
            page.height(),
            image::ExtendedColorType::Rgb8,
        )?;
        "png"
    } else {
        let quality = options.jpeg_quality();
        bytes = crate::jpeg::encode(
            page.as_raw(),
            page.width(),
            page.height(),
            crate::jpeg::Samples::Rgb,
            quality,
        )?;
        "jpg"
    };
    Ok(ProcessedPage {
        extension: extension.to_string(),
        bytes,
        black_background: fill_is_black(options, background),
        role,
        source_truncated: false,
    })
}
