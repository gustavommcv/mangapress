//! Per-page processing orchestration — the equivalent of KCC's
//! `ComicPageParser`/`ComicPage` (`image.py`) and `imgFileProcessing()`
//! (`comic2ebook.py`).

pub mod spread;

use crate::crop::{self, Background, CropPolicy};
use crate::error::Result;
use crate::fill_check::fill_check;
use crate::quantize::Container;
use crate::resize::{self, ResizeOptions};
use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat};

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
        }]);
    }

    let decoded = crate::input::decode_image(source_bytes)?;
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
        outputs.push(finish_page(
            variant, role, target, options, background, is_color,
        )?);
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

    let mut bytes = Vec::new();
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
                image::DynamicImage::ImageLuma8(gray)
                    .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)?;
                "png"
            }
        }
    } else {
        // Not `DynamicImage::write_to(..., ImageFormat::Jpeg)`: that always
        // encodes at the `image` crate's own default quality of 75,
        // regardless of device profile -- noticeably more compressed than
        // KCC's own 85/90 default for every page this pipeline produces.
        let quality = options.jpeg_quality();
        JpegEncoder::new_with_quality(&mut std::io::Cursor::new(&mut bytes), quality).write_image(
            page.as_raw(),
            page.width(),
            page.height(),
            ExtendedColorType::L8,
        )?;
        "jpg"
    };
    Ok(ProcessedPage {
        extension: extension.to_string(),
        bytes,
        black_background: fill_is_black(options, background),
        role,
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

    let mut bytes = Vec::new();
    let extension = if options.force_png && options.force_png_rgb {
        image::DynamicImage::ImageRgb8(page)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)?;
        "png"
    } else {
        let quality = options.jpeg_quality();
        JpegEncoder::new_with_quality(&mut std::io::Cursor::new(&mut bytes), quality).write_image(
            page.as_raw(),
            page.width(),
            page.height(),
            ExtendedColorType::Rgb8,
        )?;
        "jpg"
    };
    Ok(ProcessedPage {
        extension: extension.to_string(),
        bytes,
        black_background: fill_is_black(options, background),
        role,
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
mod tests {
    use super::*;

    #[test]
    fn scribe_and_colorsoft_profiles_default_to_90() {
        assert_eq!(
            default_jpeg_quality(crate::profile::Profile::by_code("KS").unwrap()),
            90
        );
        assert_eq!(
            default_jpeg_quality(crate::profile::Profile::by_code("KCS").unwrap()),
            90
        );
        assert_eq!(
            default_jpeg_quality(crate::profile::Profile::by_code("KS3").unwrap()),
            90
        );
    }

    #[test]
    fn other_profiles_default_to_85() {
        assert_eq!(
            default_jpeg_quality(crate::profile::Profile::by_code("KV").unwrap()),
            85
        );
    }

    #[test]
    fn a_custom_size_gives_sixteen_gray_levels_whatever_the_device_had() {
        use crate::profile::{Palette, Profile};
        let k1 = Profile::by_code("K1").unwrap();
        let k2 = Profile::by_code("K2").unwrap();
        assert_eq!(effective_palette(k1, None, None), Palette::Gray4);
        assert_eq!(effective_palette(k2, None, None), Palette::Gray15);
        for (width, height) in [
            (Some(800), None),
            (None, Some(1200)),
            (Some(800), Some(1200)),
        ] {
            assert_eq!(effective_palette(k1, width, height), Palette::Gray16);
            assert_eq!(effective_palette(k2, width, height), Palette::Gray16);
        }
        // Zero is "not overridden", as it is for the resolution.
        assert_eq!(effective_palette(k1, Some(0), Some(0)), Palette::Gray4);
    }

    #[test]
    fn a_custom_size_makes_scribe_and_colorsoft_ordinary_devices_for_the_jpeg_quality() {
        use crate::profile::Profile;
        for code in ["KS", "KS3", "KCS", "KSCS"] {
            let profile = Profile::by_code(code).unwrap();
            assert_eq!(
                effective_default_jpeg_quality(profile, None, None),
                90,
                "{code}"
            );
            assert_eq!(
                effective_default_jpeg_quality(profile, Some(1000), None),
                85,
                "{code}"
            );
            assert_eq!(
                effective_default_jpeg_quality(profile, None, Some(1000)),
                85,
                "{code}"
            );
            assert_eq!(
                effective_default_jpeg_quality(profile, Some(0), Some(0)),
                90,
                "{code}"
            );
        }
        let kv = Profile::by_code("KV").unwrap();
        assert_eq!(
            effective_default_jpeg_quality(kv, Some(1000), Some(1000)),
            85
        );
    }

    #[test]
    fn the_quality_asked_for_wins_over_every_default() {
        let mut options = options();
        options.profile = crate::profile::Profile::by_code("KS3").unwrap();
        options.jpeg_quality = Some(60);
        assert_eq!(options.jpeg_quality(), 60);
        options.width_override = Some(1000);
        assert_eq!(options.jpeg_quality(), 60);
        options.jpeg_quality = None;
        assert_eq!(options.jpeg_quality(), 85);
        options.width_override = None;
        assert_eq!(options.jpeg_quality(), 90);
    }

    fn options() -> PipelineOptions {
        PipelineOptions {
            profile: crate::profile::Profile::by_code("K11").unwrap(),
            width_override: None,
            height_override: None,
            manga_style: false,
            cropping: CroppingMode::Margins,
            cropping_power: 1.0,
            cropping_minimum: 0.0,
            preserve_margin_percent: 0.0,
            inter_panel_crop: crate::crop::inter_panel::InterPanelMode::Disabled,
            splitter: SplitterMode::Split,
            // Off, with pages smaller than the device, so nothing is resized
            // and output sizes are exactly the cropped sizes.
            upscale: false,
            stretch: false,
            wallpaper: false,
            white_borders: false,
            black_borders: false,
            rotate_right: false,
            no_rotate: false,
            rotate_first: false,
            maximize_strips: false,
            color_autocontrast: false,
            webtoon: false,
            force_color: false,
            force_png_rgb: false,
            png_legacy: false,
            no_quantize: false,
            no_processing: false,
            output_format: OutputFormat::Epub,
            force_png: false,
            gamma: None,
            autolevel: false,
            noautocontrast: false,
            erase_rainbow: false,
            jpeg_quality: Some(100),
        }
    }

    #[test]
    fn kindle_dx_cbz_target_does_not_change_the_device_table_or_other_formats() {
        let mut options = options();
        options.profile = crate::profile::Profile::by_code("KDX").unwrap();
        assert_eq!(
            options.profile.effective_resolution(None, None),
            (824, 1000)
        );
        for (format, expected) in [
            (OutputFormat::Cbz, (824, 1200)),
            (OutputFormat::Epub, (824, 1000)),
            (OutputFormat::Pdf, (824, 1000)),
        ] {
            options.output_format = format;
            assert_eq!(options.target_resolution(), expected);
        }
        for profile in crate::profile::PROFILES.iter().filter(|p| p.code != "KDX") {
            for format in [OutputFormat::Cbz, OutputFormat::Epub, OutputFormat::Pdf] {
                let expected = match (format, profile.code) {
                    (OutputFormat::Epub, "KS3" | "KSCS") => (1920, 2648),
                    _ => (profile.width, profile.height),
                };
                assert_eq!(format.target_resolution(profile, None, None), expected);
            }
        }
    }

    #[test]
    fn either_custom_dimension_disables_the_kindle_dx_cbz_target() {
        let mut options = options();
        options.profile = crate::profile::Profile::by_code("KDX").unwrap();
        options.output_format = OutputFormat::Cbz;
        for (width, height, expected) in [
            (Some(824), None, (824, 1000)),
            (None, Some(1000), (824, 1000)),
            (Some(900), None, (900, 1000)),
            (None, Some(1400), (824, 1400)),
            (Some(900), Some(1400), (900, 1400)),
        ] {
            options.width_override = width;
            options.height_override = height;
            assert_eq!(options.target_resolution(), expected);
        }
    }

    #[test]
    fn scribe_epub_width_cap_does_not_apply_to_other_formats_or_custom_dimensions() {
        let mut options = options();
        for code in ["KS3", "KSCS"] {
            options.profile = crate::profile::Profile::by_code(code).unwrap();
            assert_eq!(
                (options.profile.width, options.profile.height),
                (1986, 2648)
            );
            for (format, expected) in [
                (OutputFormat::Epub, (1920, 2648)),
                (OutputFormat::Cbz, (1986, 2648)),
                (OutputFormat::Pdf, (1986, 2648)),
            ] {
                options.output_format = format;
                options.width_override = None;
                options.height_override = None;
                assert_eq!(options.target_resolution(), expected);
                options.width_override = Some(1986);
                assert_eq!(options.target_resolution(), (1986, 2648));
                options.width_override = None;
                options.height_override = Some(2648);
                assert_eq!(options.target_resolution(), (1986, 2648));
            }
        }
    }

    fn png(page: image::RgbImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(page)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn decode(page: &ProcessedPage) -> image::GrayImage {
        image::load_from_memory(&page.bytes).unwrap().to_luma8()
    }

    /// A white page with a block of content inset by `margin` on every side:
    /// dark gray (40), a lighter band (200) through it, and — if `colored` —
    /// a saturated patch, which is what makes upstream call a page color.
    fn page_with_margins(
        (width, height): (u32, u32),
        margin: u32,
        colored: bool,
    ) -> image::RgbImage {
        image::RgbImage::from_fn(width, height, |x, y| {
            let inside = x >= margin && x < width - margin && y >= margin && y < height - margin;
            if !inside {
                image::Rgb([255, 255, 255])
            } else if colored && x < width / 2 && y < height / 3 {
                image::Rgb([200, 60, 60])
            } else if y % 40 < 10 {
                image::Rgb([200, 200, 200])
            } else {
                image::Rgb([40, 40, 40])
            }
        })
    }

    fn extrema(page: &image::GrayImage) -> (u8, u8) {
        page.pixels()
            .fold((255, 0), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])))
    }

    #[test]
    fn a_color_page_is_not_autocontrasted_but_a_gray_one_is() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        // No margins: the page's values run 40..200, wide enough (160) that
        // autocontrast would stretch them to 0..255.
        let gray = process_page(
            &png(page_with_margins((300, 450), 0, false)),
            &options,
            false,
        )
        .unwrap();
        let (low, high) = extrema(&decode(&gray[0]));
        assert!(low < 10 && high > 245, "gray page stretched: {low}..{high}");

        let color = process_page(
            &png(page_with_margins((300, 450), 0, true)),
            &options,
            false,
        )
        .unwrap();
        let (low, high) = extrema(&decode(&color[0]));
        assert!(
            low > 25 && high < 215,
            "color page left at its own contrast: {low}..{high}"
        );
    }

    #[test]
    fn a_color_first_page_is_left_uncropped() {
        let cover = png(page_with_margins((400, 600), 30, true));
        let first = process_page(&cover, &options(), true).unwrap();
        assert_eq!(decode(&first[0]).dimensions(), (400, 600));

        // The same page anywhere else in the book loses its margins...
        let later = process_page(&cover, &options(), false).unwrap();
        let (width, height) = decode(&later[0]).dimensions();
        assert!(width < 350 && height < 550, "{width}x{height}");

        // ...and so does a first page that isn't color.
        let gray_first = process_page(
            &png(page_with_margins((400, 600), 30, false)),
            &options(),
            true,
        )
        .unwrap();
        assert!(decode(&gray_first[0]).width() < 350);
    }

    #[test]
    fn margins_are_cropped_before_a_spread_is_split() {
        // Two 900px-wide pages side by side with 150px of margin on the left
        // and 50px on the right. Cropping first leaves 1800px, split down the
        // gutter into two equal halves; splitting first would cut 50px into
        // the left page and give halves of different widths.
        let spread = image::RgbImage::from_fn(2000, 1400, |x, y| {
            let inside = (150..1950).contains(&x) && (25..1375).contains(&y);
            if inside {
                image::Rgb([60, 60, 60])
            } else {
                image::Rgb([255, 255, 255])
            }
        });
        let halves = process_page(&png(spread), &options(), false).unwrap();
        assert_eq!(halves.len(), 2);
        let (first, second) = (decode(&halves[0]), decode(&halves[1]));
        assert_eq!(first.width(), second.width());
        assert!((899..=901).contains(&first.width()), "{}", first.width());
        assert_eq!(
            (halves[0].role, halves[1].role),
            (spread::PageRole::SplitFirst, spread::PageRole::SplitSecond)
        );
    }

    #[test]
    fn a_dark_page_is_flagged_for_a_black_page_background_unless_borders_are_forced_white() {
        let dark = image::RgbImage::from_fn(300, 450, |x, y| {
            if (100..200).contains(&x) && (150..300).contains(&y) {
                image::Rgb([230, 230, 230])
            } else {
                image::Rgb([10, 10, 10])
            }
        });
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        let page = process_page(&png(dark.clone()), &options, false).unwrap();
        assert!(page[0].black_background);

        options.white_borders = true;
        let page = process_page(&png(dark), &options, false).unwrap();
        assert!(!page[0].black_background);

        let light = process_page(
            &png(page_with_margins((300, 450), 20, false)),
            &options,
            false,
        )
        .unwrap();
        assert!(!light[0].black_background);
    }

    #[test]
    fn a_quantized_page_is_a_palette_png_unless_upstream_makes_it_grayscale() {
        let container = |profile: &str, format: OutputFormat, custom_width: Option<u32>| {
            let mut options = options();
            options.profile = crate::profile::Profile::by_code(profile).unwrap();
            options.output_format = format;
            options.width_override = custom_width;
            quantized_container(&options)
        };
        // EPUB and CBZ are palette PNG on every device — a Kindle included,
        // where upstream would write a GIF.
        for profile in ["K11", "K2", "KoC", "Rmk2"] {
            assert_eq!(
                container(profile, OutputFormat::Epub, None),
                Container::IndexedPng,
                "{profile}"
            );
        }
        assert_eq!(
            container("K11", OutputFormat::Cbz, None),
            Container::IndexedPng
        );
        // PDF, and CBZ for the four oldest Kindles, go back to grayscale...
        assert_eq!(
            container("K11", OutputFormat::Pdf, None),
            Container::GrayPng
        );
        for profile in ["K1", "K2", "K34", "KDX"] {
            assert_eq!(
                container(profile, OutputFormat::Cbz, None),
                Container::GrayPng
            );
        }
        // ...unless the resolution is custom, which upstream no longer
        // treats as one of those four.
        assert_eq!(
            container("K2", OutputFormat::Cbz, Some(1000)),
            Container::IndexedPng
        );
    }

    #[test]
    fn force_png_writes_png_with_the_same_pixels_in_either_container() {
        let page = png(page_with_margins((300, 450), 20, false));
        let mut options = options();
        options.force_png = true;

        // A Kindle profile's EPUB: palette PNG, 4 bits per pixel.
        let indexed = process_page(&page, &options, false).unwrap();
        assert_eq!(indexed[0].extension, "png");
        assert_eq!(&indexed[0].bytes[1..4], b"PNG");
        assert_eq!(indexed[0].bytes[24], 4, "bit depth");
        assert_eq!(indexed[0].bytes[25], 3, "color type: palette");

        options.output_format = OutputFormat::Pdf;
        let gray = process_page(&page, &options, false).unwrap();
        assert_eq!(gray[0].bytes[24], 8, "bit depth");
        assert_eq!(decode(&gray[0]), decode(&indexed[0]));
        let levels = options.profile.palette.level_values();
        assert!(decode(&gray[0]).pixels().all(|p| levels.contains(&p[0])));
    }

    /// A 2000x1400 spread with content edge to edge: its left half dark,
    /// its right half light, so the halves can be told apart.
    fn two_tone_spread() -> image::RgbImage {
        image::RgbImage::from_fn(2000, 1400, |x, _| {
            if x < 1000 {
                image::Rgb([40, 40, 40])
            } else {
                image::Rgb([200, 200, 200])
            }
        })
    }

    fn mean(page: &image::GrayImage) -> f64 {
        page.pixels().map(|p| p[0] as f64).sum::<f64>() / (page.width() * page.height()) as f64
    }

    #[test]
    fn rotate_first_puts_the_whole_spread_before_its_halves() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.splitter = SplitterMode::Both;
        let roles = |options: &PipelineOptions| -> Vec<spread::PageRole> {
            process_page(&png(two_tone_spread()), options, false)
                .unwrap()
                .iter()
                .map(|page| page.role)
                .collect()
        };
        use spread::PageRole::{Rotated, SplitFirst, SplitSecond};
        assert_eq!(roles(&options), [SplitFirst, SplitSecond, Rotated]);
        options.rotate_first = true;
        assert_eq!(roles(&options), [Rotated, SplitFirst, SplitSecond]);
    }

    #[test]
    fn no_rotate_keeps_the_whole_spread_upright_and_only_shrinks_it_when_oversized() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.splitter = SplitterMode::Rotate;
        options.upscale = true;
        options.no_rotate = true;
        // Kobo: 1072x1448. A 2000x1400 spread fits two widths by one height,
        // so it is left exactly as it is, landscape.
        options.profile = crate::profile::Profile::by_code("KoC").unwrap();
        let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
        assert_eq!(page[0].role, spread::PageRole::Rotated);
        assert_eq!(decode(&page[0]).dimensions(), (2000, 1400));

        // The same on a Kindle profile, where upstream would cap it at 1920px.
        options.profile = crate::profile::Profile::by_code("K11").unwrap();
        let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
        assert_eq!(decode(&page[0]).dimensions(), (2000, 1400));

        // Larger than two screens wide, it is shrunk to exactly that.
        let huge = image::RgbImage::from_pixel(4288, 1448, image::Rgb([128, 128, 128]));
        let page = process_page(&png(huge), &options, false).unwrap();
        assert_eq!(decode(&page[0]).dimensions(), (2144, 724));

        // Rotated as usual without the flag.
        options.no_rotate = false;
        let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
        let (width, height) = decode(&page[0]).dimensions();
        assert!(height > width);
    }

    #[test]
    fn maximize_strips_stacks_the_first_read_half_on_top() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.noautocontrast = true;
        options.maximize_strips = true;
        let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
        assert_eq!(page.len(), 1, "no spread handling at all");
        assert_eq!(page[0].role, spread::PageRole::Normal);
        let stacked = decode(&page[0]);
        // 1000x2800, fitted into 1072x1448.
        assert_eq!(stacked.dimensions(), (517, 1448));
        let top = image::imageops::crop_imm(&stacked, 0, 0, 517, 700).to_image();
        let bottom = image::imageops::crop_imm(&stacked, 0, 748, 517, 700).to_image();
        // Left-to-right: the dark left half on top.
        assert!(mean(&top) < 60.0 && mean(&bottom) > 180.0);

        options.manga_style = true;
        let stacked = decode(&process_page(&png(two_tone_spread()), &options, false).unwrap()[0]);
        let top = image::imageops::crop_imm(&stacked, 0, 0, 517, 700).to_image();
        assert!(mean(&top) > 180.0, "right-to-left: the right half on top");
    }

    #[test]
    fn black_borders_flag_every_page_and_win_over_white_borders() {
        let light = png(page_with_margins((300, 450), 20, false));
        let mut options = options();
        options.black_borders = true;
        assert!(process_page(&light, &options, false).unwrap()[0].black_background);
        options.white_borders = true;
        assert!(process_page(&light, &options, false).unwrap()[0].black_background);
    }

    #[test]
    fn color_autocontrast_stretches_a_color_page_too() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.color_autocontrast = true;
        let color = process_page(
            &png(page_with_margins((300, 450), 0, true)),
            &options,
            false,
        )
        .unwrap();
        let (low, high) = extrema(&decode(&color[0]));
        assert!(low < 10 && high > 245, "{low}..{high}");
    }

    #[test]
    fn png_legacy_and_no_quantize_store_plain_grayscale() {
        let container = |format: OutputFormat, legacy: bool, no_quantize: bool| {
            let mut options = options();
            options.output_format = format;
            options.png_legacy = legacy;
            options.no_quantize = no_quantize;
            quantized_container(&options)
        };
        assert_eq!(
            container(OutputFormat::Epub, true, false),
            Container::GrayPng
        );
        assert_eq!(
            container(OutputFormat::Epub, false, true),
            Container::GrayPng
        );
        assert_eq!(
            container(OutputFormat::Cbz, true, false),
            Container::GrayPng
        );
        assert_eq!(
            container(OutputFormat::Cbz, false, true),
            Container::GrayPng
        );

        let page = png(page_with_margins((300, 450), 20, false));
        let mut options = options();
        options.force_png = true;
        options.png_legacy = true;
        let legacy = process_page(&page, &options, false).unwrap();
        assert_eq!(legacy[0].extension, "png");
        assert_eq!(legacy[0].bytes[24], 8, "bit depth");
        let levels = options.profile.palette.level_values();
        assert!(decode(&legacy[0]).pixels().all(|p| levels.contains(&p[0])));

        options.png_legacy = false;
        options.no_quantize = true;
        let unquantized = decode(&process_page(&page, &options, false).unwrap()[0]);
        assert!(
            unquantized.pixels().any(|p| !levels.contains(&p[0])),
            "all 256 levels kept"
        );
    }

    #[test]
    fn no_processing_hands_the_source_back_untouched() {
        let source = png(page_with_margins((300, 450), 20, true));
        let mut options = options();
        options.no_processing = true;
        let page = process_page(&source, &options, true).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].extension, "png");
        assert_eq!(page[0].bytes, source);
    }

    #[test]
    fn bmp_passthrough_keeps_its_codec_except_in_epub() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            32,
            48,
            image::Rgb([40, 80, 160]),
        ))
        .write_to(&mut encoded, ImageFormat::Bmp)
        .unwrap();
        let source = encoded.into_inner();
        let mut options = options();
        options.no_processing = true;
        for format in [OutputFormat::Cbz, OutputFormat::Pdf] {
            options.output_format = format;
            let pages = process_page(&source, &options, true).unwrap();
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].extension, "bmp");
            assert_eq!(pages[0].bytes, source);
        }
        options.output_format = OutputFormat::Epub;
        let error = process_page(&source, &options, true).unwrap_err();
        assert!(matches!(error, crate::Error::Encode(_)));
        assert!(error
            .to_string()
            .contains("Remove --noprocessing or choose CBZ"));
        options.no_processing = false;
        assert!(process_page(&source, &options, true).is_ok());
    }

    #[test]
    fn oversized_images_are_refused_in_normal_and_passthrough_modes() {
        let source = crate::test_support::oversized_bmp();
        for no_processing in [false, true] {
            let mut options = options();
            options.no_processing = no_processing;
            assert!(matches!(
                process_page(&source, &options, true),
                Err(crate::Error::ImageTooLarge { .. })
            ));
        }
    }

    #[test]
    fn force_color_keeps_a_color_page_in_color_and_a_gray_page_gray() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.force_color = true;

        let color = process_page(
            &png(page_with_margins((300, 450), 0, true)),
            &options,
            false,
        )
        .unwrap();
        let decoded = image::load_from_memory(&color[0].bytes).unwrap();
        assert!(decoded.color().has_color());
        // The saturated patch is still red: far more red than green.
        let patch = decoded.to_rgb8().get_pixel(40, 40).0;
        assert!(patch[0] > 170 && patch[1] < 90, "{patch:?}");

        let gray = process_page(
            &png(page_with_margins((300, 450), 0, false)),
            &options,
            false,
        )
        .unwrap();
        assert!(!image::load_from_memory(&gray[0].bytes)
            .unwrap()
            .color()
            .has_color());
    }

    #[test]
    fn force_png_rgb_saves_a_color_page_as_png() {
        let mut options = options();
        options.cropping = CroppingMode::Disabled;
        options.force_color = true;
        options.force_png = true;
        options.output_format = OutputFormat::Cbz;
        let page = png(page_with_margins((300, 450), 0, true));
        // Without it, a color page stays JPEG even under --forcepng.
        assert_eq!(
            process_page(&page, &options, false).unwrap()[0].extension,
            "jpg"
        );
        options.force_png_rgb = true;
        let saved = process_page(&page, &options, false).unwrap();
        assert_eq!(saved[0].extension, "png");
        assert!(image::load_from_memory(&saved[0].bytes)
            .unwrap()
            .color()
            .has_color());
    }

    #[test]
    fn webtoon_pages_are_never_margin_cropped_split_or_autocontrasted() {
        let mut options = options();
        options.webtoon = true;
        options.splitter = SplitterMode::Both;

        // Wide enough to be a spread, and with contrast autocontrast would
        // stretch (40..200): it comes out as one page, shrunk to the
        // device's width, at its own contrast.
        let out = process_page(&png(two_tone_spread()), &options, false).unwrap();
        assert_eq!(out.len(), 1);
        let page = decode(&out[0]);
        assert_eq!(page.dimensions(), (1072, 750));
        let (low, high) = extrema(&page);
        assert!(low > 15 && high < 225, "{low}..{high}");

        // Margins stay.
        let with_margins = png(page_with_margins((400, 600), 30, false));
        let out = process_page(&with_margins, &options, false).unwrap();
        assert_eq!(decode(&out[0]).dimensions(), (400, 600));
    }
}
