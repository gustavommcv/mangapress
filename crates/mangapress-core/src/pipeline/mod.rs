//! Per-page processing orchestration — the equivalent of KCC's
//! `ComicPageParser`/`ComicPage` (`image.py`) and `imgFileProcessing()`
//! (`comic2ebook.py`).

pub mod spread;

use crate::crop::{self, Background, CropPolicy};
use crate::error::Result;
use crate::fill_check::fill_check;
use crate::quantize::Container;
use crate::resize::{self, ResizeOptions};
use image::ImageFormat;

/// Options that drive a single conversion run. Mirrors the relevant subset
/// of `kcc-c2e.py`'s argument groups (MAIN/PROCESSING) — see
/// `mangapress-cli`'s `args.rs` for the full CLI surface and which of these
/// are already wired up vs still `todo!()` downstream.
#[derive(Debug, Clone)]
pub struct PipelineOptions {
    pub profile: &'static crate::profile::Profile,
    /// `--customwidth`/`--customheight`: independently override either
    /// dimension of `profile`'s resolution. See
    /// [`crate::profile::Profile::effective_resolution`].
    pub width_override: Option<u32>,
    pub height_override: Option<u32>,
    pub manga_style: bool,
    pub cropping: CroppingMode,
    /// `--croppingpower`. Higher power crops through more.
    pub cropping_power: f32,
    /// `--croppingminimum`, 0-100: only actually crop if doing so would keep
    /// at least this percentage of the page's area. Converted to
    /// [`crate::crop::CropPolicy::minimum_area_ratio`]'s 0.0-1.0 fraction in
    /// [`crop_policy`].
    pub cropping_minimum: f32,
    /// `--preservemargin`, 0-100: back the computed crop off by this
    /// percentage after the 10% cap, so *some* margin is deliberately kept.
    pub preserve_margin_percent: f32,
    pub inter_panel_crop: crate::crop::inter_panel::InterPanelMode,
    pub splitter: SplitterMode,
    pub upscale: bool,
    pub stretch: bool,
    pub wallpaper: bool,
    pub white_borders: bool,
    /// `--blackborders`: treat every page's surroundings as black, whatever
    /// its detected background — the pad color for CBZ/PDF and the page
    /// background in an EPUB. Wins over `white_borders` for the color;
    /// `white_borders` still decides *whether* CBZ/PDF pages are padded.
    pub black_borders: bool,
    /// `--rotateright`: rotate spreads clockwise instead of the default
    /// counter-clockwise. See [`spread::execute`].
    pub rotate_right: bool,
    /// `--norotate`: keep the whole-spread copy upright instead of turning
    /// it. An upright spread is not fitted to the device like a page: it is
    /// only shrunk, and only when it exceeds two device widths by one device
    /// height (see [`resize_upright_spread`]).
    pub no_rotate: bool,
    /// `--rotatefirst`: in `Both` mode, put the whole-spread copy before the
    /// two halves instead of after them.
    pub rotate_first: bool,
    /// `--maximizestrips`: restack every page's two halves on top of each
    /// other (a 1x4 strip becomes 2x2) instead of looking for a spread.
    pub maximize_strips: bool,
    /// `--colorautocontrast`: autocontrast color pages too.
    pub color_autocontrast: bool,
    /// `--webtoon`: the pages are slices of a vertical strip (see
    /// [`crate::webtoon`], which the caller runs first). They are never
    /// margin-cropped, never treated as spreads, never autocontrasted, and
    /// always count as color pages. Upstream also forces left-to-right
    /// order, white borders and no upscaling in this mode; the caller sets
    /// those fields accordingly.
    pub webtoon: bool,
    /// `--forcecolor`: keep color pages in color. A page upstream's color
    /// test calls gray is still converted to grayscale and goes through the
    /// ordinary path.
    pub force_color: bool,
    /// `--force-png-rgb`: with `force_png` and `force_color`, save color
    /// pages as PNG too, instead of leaving them as JPEG.
    pub force_png_rgb: bool,
    /// `--pnglegacy`: with `force_png`, store the quantized page as 8-bit
    /// grayscale instead of at the palette's own bit depth.
    pub png_legacy: bool,
    /// `--noquantize`: with `force_png`, keep all 256 gray levels.
    pub no_quantize: bool,
    /// `--noprocessing`: leave every image exactly as it is.
    /// [`process_page`] hands the source bytes back untouched.
    /// EPUB cannot embed BMP unchanged; that combination is refused.
    pub no_processing: bool,
    pub output_format: OutputFormat,
    /// `--forcepng`: quantize to the profile's grayscale palette (Floyd-
    /// Steinberg dithered, as Pillow dithers) instead of full-tone JPEG, and
    /// store the result as a palette PNG — or as 8-bit grayscale where
    /// upstream does; see [`crate::quantize::Container`].
    pub force_png: bool,
    pub gamma: Option<f32>,
    /// `--autolevel`: run [`crate::contrast::autolevel`] before autocontrast.
    pub autolevel: bool,
    /// `--noautocontrast`: skip autocontrast entirely.
    pub noautocontrast: bool,
    /// `--eraserainbow`: run [`crate::rainbow::erase_rainbow_artifacts_gray`]
    /// after resize.
    pub erase_rainbow: bool,
    /// `--jpeg-quality`, 1-100. `None` means "use [`default_jpeg_quality`]
    /// for this profile", matching KCC's own `checkOptions()` default.
    pub jpeg_quality: Option<u8>,
}

/// KCC's own default JPEG quality (`checkOptions()`, confirmed by reading
/// the source): Kindle Scribe and Colorsoft profiles get 90, everything
/// else gets 85. Only applies when `--jpeg-quality` isn't passed.
pub fn default_jpeg_quality(profile: &crate::profile::Profile) -> u8 {
    if profile.code.starts_with("KS") || profile.code == "KCS" {
        90
    } else {
        85
    }
}

/// Whether either dimension is overridden. Upstream then swaps the device for a profile of its
/// own, "Custom", which keeps only the device's other settings: it has sixteen gray levels
/// whatever the device had, and, being no Scribe or Colorsoft, the default JPEG quality of an
/// ordinary device.
pub fn is_custom_size(width_override: Option<u32>, height_override: Option<u32>) -> bool {
    width_override.unwrap_or(0) != 0 || height_override.unwrap_or(0) != 0
}

/// The gray levels of the pages: the device's own, or sixteen with a custom size.
pub fn effective_palette(
    profile: &crate::profile::Profile,
    width_override: Option<u32>,
    height_override: Option<u32>,
) -> crate::profile::Palette {
    if is_custom_size(width_override, height_override) {
        crate::profile::Palette::Gray16
    } else {
        profile.palette
    }
}

/// [`default_jpeg_quality`], except that a custom size makes the device an ordinary one.
pub fn effective_default_jpeg_quality(
    profile: &crate::profile::Profile,
    width_override: Option<u32>,
    height_override: Option<u32>,
) -> u8 {
    if is_custom_size(width_override, height_override) {
        85
    } else {
        default_jpeg_quality(profile)
    }
}

impl PipelineOptions {
    /// The gray levels of the pages (see [`effective_palette`]).
    pub fn palette(&self) -> crate::profile::Palette {
        effective_palette(self.profile, self.width_override, self.height_override)
    }

    /// The JPEG quality of the pages: the one asked for, or the default (see
    /// [`effective_default_jpeg_quality`]).
    pub fn jpeg_quality(&self) -> u8 {
        self.jpeg_quality.unwrap_or_else(|| {
            effective_default_jpeg_quality(self.profile, self.width_override, self.height_override)
        })
    }

    /// The format-specific target, or the custom resolution when supplied.
    pub fn target_resolution(&self) -> (u32, u32) {
        self.output_format.target_resolution(
            self.profile,
            self.width_override,
            self.height_override,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CroppingMode {
    Disabled,
    Margins,
    MarginsAndPageNumbers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitterMode {
    Split,
    Rotate,
    Both,
}

/// Mirrors `mangapress-cli`'s `Format` enum — duplicated rather than
/// depended-on, since `mangapress-core` cannot depend on the binary crate.
/// Only affects processing decisions here (e.g. resize's pad-vs-contain
/// choice); actual ebook assembly is a separate, later step the CLI drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Epub,
    Cbz,
    Pdf,
}

impl OutputFormat {
    /// Resolve the processing target without changing the device table.
    /// KCC 12.0.0's `checkOptions()` uses a 1200-pixel CBZ height for Kindle
    /// DX and caps Scribe EPUB width at 1920. Either custom dimension
    /// disables those adjustments, even a value equal to the built-in one.
    pub fn target_resolution(
        self,
        profile: &crate::profile::Profile,
        width_override: Option<u32>,
        height_override: Option<u32>,
    ) -> (u32, u32) {
        if width_override.is_some() || height_override.is_some() {
            return profile.effective_resolution(width_override, height_override);
        }
        match self {
            Self::Cbz if profile.code == "KDX" => (profile.width, 1200),
            Self::Epub if profile.code.starts_with("KS") => {
                (profile.width.min(1920), profile.height)
            }
            _ => (profile.width, profile.height),
        }
    }
}

/// One page this pipeline produced, encoded and ready for an ebook builder,
/// with the two things a builder needs to know about it beyond its pixels.
#[derive(Debug, Clone)]
pub struct ProcessedPage {
    /// Lowercase, no leading dot (`"jpg"`, `"png"`).
    pub extension: String,
    pub bytes: Vec<u8>,
    /// The source page's background was detected as dark and
    /// `--whiteborders` doesn't overrule it — upstream's `BlackBackground`
    /// page flag, which makes the EPUB paint the area around the page black
    /// instead of leaving it white.
    pub black_background: bool,
    pub role: spread::PageRole,
    /// The source page's file ended, or its data broke, before the whole image was read, and
    /// what was not read is blank (see [`crate::input`]): every piece of that page says so.
    pub source_truncated: bool,
}

/// Processes a single source page, in upstream KCC 12.0.0's order:
/// decode -> detect background and color -> crop margins (and page number)
/// -> inter-panel crop -> spread decide/execute -> (per resulting page)
/// gamma -> grayscale -> autocontrast -> resize -> rainbow-artifact removal
/// (if `--eraserainbow`) -> quantize (if `--forcepng`) -> encode.
///
/// Three things here follow upstream changes made after the commit this
/// crate was first written against, each confirmed by running real KCC
/// 12.0.0 on the same input rather than read off its changelog:
/// - Cropping happens *before* a spread is split, on the whole source page,
///   not on each half afterwards. The split line is the middle of the
///   cropped page, so a spread with uneven outer margins is cut at its
///   gutter instead of beside it: 150px of margin on one side and 50px on
///   the other used to give halves 1019px and 964px wide where upstream
///   gives two of 964px.
/// - `is_first_page` — the book's first page — is left uncropped when it is
///   a color page: a cover, whose art runs to the edge by design.
/// - A color page (see [`crate::color::has_meaningful_color`]) is not
///   autocontrasted, though it still ends up grayscale.
///
/// The geometric stages run on the page's RGB pixels, steered by a
/// grayscale proxy; only gamma onward works in grayscale. That is upstream's
/// own split, and it is what lets gamma be applied per channel before the
/// conversion to gray, as upstream applies it.
///
/// Every stage here has its own fixture-backed tests in [`spread`],
/// [`crate::crop`], [`crate::color`], [`crate::contrast`],
/// [`crate::resize`], [`crate::rainbow`], and [`crate::quantize`]; this
/// function's job is only to wire already-validated pieces together in the
/// right order, not to introduce new heuristics of its own.
pub fn process_page(
    source_bytes: &[u8],
    options: &PipelineOptions,
    is_first_page: bool,
) -> Result<Vec<ProcessedPage>> {
    if options.no_processing {
        crate::input::image_dimensions(source_bytes)?;
        let extension = match image::guess_format(source_bytes)? {
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Png => "png",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
            ImageFormat::Bmp if options.output_format == OutputFormat::Epub => {
                return Err(crate::Error::Encode(
                    "BMP pages cannot be embedded in EPUB without processing. Remove --noprocessing or choose CBZ output."
                        .to_string(),
                ));
            }
            ImageFormat::Bmp => "bmp",
            format => {
                return Err(crate::Error::Encode(format!(
                    "unsupported passthrough image format: {format:?}"
                )));
            }
        };
        return Ok(vec![ProcessedPage {
            extension: extension.to_string(),
            bytes: source_bytes.to_vec(),
            black_background: false,
            role: spread::PageRole::Normal,
            source_truncated: false,
        }]);
    }

    let crate::input::DecodedPage {
        image: decoded,
        truncated,
    } = crate::input::decode_page(source_bytes)?;
    // Upstream answers "not color" for a single-channel source without
    // measuring it.
    let single_channel = !decoded.color().has_color();
    let page = decoded.to_rgb8();
    let target = options.target_resolution();

    // Detected once per *source* page, before any cropping or
    // spread-splitting, and reused for every resulting piece -- matching
    // upstream, which computes `fillCheck()` once in
    // `ComicPageParser.__init__` and carries that same value through every
    // payload entry `splitCheck()` produces.
    let proxy = crate::color::to_gray(&page);
    let background = fill_check(&proxy);
    // Asking for color output changes the test itself, not just what is
    // done with its answer (see `color::has_color_worth_keeping`).
    let is_color = !single_channel
        && if options.webtoon {
            true
        } else if options.force_color {
            crate::color::has_color_worth_keeping(&page)
        } else {
            crate::color::has_meaningful_color(&page)
        };

    let page = if is_first_page && is_color {
        page
    } else {
        crop_whole_page(page, proxy, options, background)
    };

    // `--maximizestrips` replaces spread handling altogether: upstream tests
    // it first and emits the restacked page as an ordinary one.
    let (page, decision) = if options.webtoon {
        (page, spread::Decision::Normal)
    } else if options.maximize_strips {
        (
            stack_halves(&page, options.manga_style),
            spread::Decision::Normal,
        )
    } else {
        let decision = spread::decide(page.dimensions(), target, options.splitter);
        (page, decision)
    };
    let variants = spread::execute(&page, decision, options.manga_style, options.rotate_right);

    let mut pieces: Vec<(image::RgbImage, spread::PageRole)> = variants
        .into_iter()
        .zip(spread::roles(decision).iter().copied())
        .map(|(variant, role)| {
            if role == spread::PageRole::Rotated && options.no_rotate {
                (page.clone(), role)
            } else {
                (variant, role)
            }
        })
        .collect();
    if options.rotate_first && decision == spread::Decision::Both {
        // Upstream names the whole-spread copy so that it sorts first.
        pieces.rotate_right(1);
    }

    let mut outputs = Vec::with_capacity(pieces.len());
    for (variant, role) in pieces {
        let mut output = finish_page(variant, role, target, options, background, is_color)?;
        output.source_truncated = truncated;
        outputs.push(output);
    }
    Ok(outputs)
}

/// Margin (and page-number) cropping, then inter-panel cropping, on the
/// whole source page. `proxy` is the page's grayscale, which both detectors
/// read; the page's own RGB pixels are what gets cropped.
fn crop_whole_page(
    page: image::RgbImage,
    proxy: image::GrayImage,
    options: &PipelineOptions,
    background: Background,
) -> image::RgbImage {
    // A webtoon page is only ever inter-panel cropped.
    let cropping = if options.webtoon {
        CroppingMode::Disabled
    } else {
        options.cropping
    };
    let crop_box = match cropping {
        CroppingMode::Disabled => None,
        CroppingMode::Margins => {
            crop::margin::compute_margin_crop(&proxy, &crop_policy(options, background))
        }
        CroppingMode::MarginsAndPageNumbers => {
            crop::page_number::compute_margin_crop_ignoring_page_number(
                &proxy,
                &crop_policy(options, background),
            )
        }
    };
    let (page, proxy) = match crop_box {
        Some(crop_box) => (
            crop::apply_crop(&page, crop_box),
            crop::apply_crop(&proxy, crop_box),
        ),
        None => (page, proxy),
    };

    crop::inter_panel::crop_empty_inter_panel_sections_using(
        page,
        &proxy,
        options.inter_panel_crop,
        background,
    )
}

fn finish_page(
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
fn quantized_container(options: &PipelineOptions) -> Container {
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

/// What surrounds a page is black: `--blackborders` says so, or the page's
/// own background is dark and `--whiteborders` doesn't overrule it.
/// Upstream's `fill`, which decides both the pad color and the EPUB page
/// background.
fn fill_is_black(options: &PipelineOptions, background: Background) -> bool {
    options.black_borders || (!options.white_borders && background == Background::Dark)
}

/// `--maximizestrips`: the page's two halves, the first-read one on top, on
/// a canvas half as wide and twice as tall. When the width is odd the right
/// half is one column wider than the canvas and loses that column, as it
/// does upstream.
fn stack_halves(page: &image::RgbImage, manga_style: bool) -> image::RgbImage {
    let (w, h) = page.dimensions();
    let half = w / 2;
    let left = image::imageops::crop_imm(page, 0, 0, half, h).to_image();
    let right = image::imageops::crop_imm(page, half, 0, w - half, h).to_image();
    let (first, second) = if manga_style {
        (right, left)
    } else {
        (left, right)
    };
    let mut stacked = image::RgbImage::new(half.max(1), h * 2);
    image::imageops::overlay(&mut stacked, &first, 0, 0);
    image::imageops::overlay(&mut stacked, &second, 0, h as i64);
    stacked
}

/// Fits a page to the device — or, for an upright whole spread, applies
/// that case's own rule. The same for a grayscale page and a color one.
fn resize_for_device<P: image::Pixel<Subpixel = u8> + 'static>(
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
fn resize_upright_spread<P: image::Pixel<Subpixel = u8> + 'static>(
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

fn crop_policy(options: &PipelineOptions, background: Background) -> CropPolicy {
    CropPolicy {
        power: options.cropping_power,
        minimum_area_ratio: (options.cropping_minimum as f64) / 100.0,
        preserve_margin_percent: options.preserve_margin_percent,
        background,
    }
}

#[cfg(test)]
mod tests;
