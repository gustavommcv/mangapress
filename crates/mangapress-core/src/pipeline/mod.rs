//! Per-page processing orchestration — the equivalent of KCC's
//! `ComicPageParser`/`ComicPage` (`image.py`) and `imgFileProcessing()`
//! (`comic2ebook.py`).

pub mod page;
pub mod spread;

use crate::crop::{self, Background, CropPolicy};
use crate::error::Result;
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
    pub cropping_power: f32,
    pub cropping_minimum: f32,
    pub inter_panel_crop: crate::crop::inter_panel::InterPanelMode,
    pub splitter: SplitterMode,
    pub upscale: bool,
    pub stretch: bool,
    pub wallpaper: bool,
    pub white_borders: bool,
    /// `--rotateright`: rotate spreads clockwise instead of the default
    /// counter-clockwise. See [`spread::execute`].
    pub rotate_right: bool,
    pub output_format: OutputFormat,
    /// `--forcepng`: quantize to the profile's grayscale palette (Floyd-
    /// Steinberg dithered) and save PNG instead of full-tone JPEG. Mirrors
    /// upstream's PNG branch of `save_with_codec()`, simplified since this
    /// pipeline is grayscale-only already (upstream's extra `not
    /// self.colorOutput` condition is therefore always true here) and
    /// MOBI/AZW3 output (upstream's GIF branch) is out of scope — see
    /// [`crate::quantize`]'s module docs for the full picture.
    pub force_png: bool,
    pub gamma: Option<f32>,
    /// `--autolevel`: run [`crate::contrast::autolevel`] before autocontrast.
    pub autolevel: bool,
    /// `--noautocontrast`: skip autocontrast entirely.
    pub noautocontrast: bool,
}

impl PipelineOptions {
    /// The resolution pages are actually resized to, after applying any
    /// `--customwidth`/`--customheight` override on top of `profile`.
    pub fn target_resolution(&self) -> (u32, u32) {
        self.profile
            .effective_resolution(self.width_override, self.height_override)
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

/// Processes a single source page: decode -> grayscale -> spread
/// decide/execute -> (per resulting page) crop -> inter-panel crop -> gamma
/// -> autocontrast -> resize -> quantize (if `--forcepng`) -> encode. A
/// double-page spread can expand into two split halves and/or a rotated
/// whole (see [`spread::execute`]) *before* cropping — matching upstream's
/// order, where spread detection runs on the original page and crop/resize
/// apply independently to each resulting piece, not the other way around.
///
/// Also not yet applied, tracked as a gap rather than silently skipped:
/// rainbow-artifact removal, which exists only as a `todo!()` still. What
/// *is* applied (spread, crop, inter-panel crop, gamma, autocontrast,
/// resize, quantize) has its own fixture-backed tests in [`spread`],
/// [`crate::crop`], [`crate::contrast`], [`crate::resize`], and
/// [`crate::quantize`]; this function's job is only to wire already-
/// validated pieces together in the right order, not to introduce new
/// heuristics of its own.
pub fn process_page(
    source_bytes: &[u8],
    options: &PipelineOptions,
) -> Result<Vec<(String, Vec<u8>)>> {
    let page = image::load_from_memory(source_bytes)?.to_luma8();
    let target = options.target_resolution();

    let decision = spread::decide(page.dimensions(), target, options.splitter);
    let variants = spread::execute(&page, decision, options.manga_style, options.rotate_right);

    let mut outputs = Vec::with_capacity(variants.len());
    for variant in variants {
        outputs.push(finish_page(variant, target, options)?);
    }
    Ok(outputs)
}

fn finish_page(
    page: image::GrayImage,
    target: (u32, u32),
    options: &PipelineOptions,
) -> Result<(String, Vec<u8>)> {
    let page = match options.cropping {
        CroppingMode::Disabled => page,
        CroppingMode::Margins => {
            let policy = crop_policy(options);
            match crop::margin::compute_margin_crop(&page, &policy) {
                Some(crop_box) => crop::apply_crop(&page, crop_box),
                None => page,
            }
        }
        CroppingMode::MarginsAndPageNumbers => {
            let policy = crop_policy(options);
            match crop::page_number::compute_margin_crop_ignoring_page_number(&page, &policy) {
                Some(crop_box) => crop::apply_crop(&page, crop_box),
                None => page,
            }
        }
    };

    let page = crop::inter_panel::crop_empty_inter_panel_sections(
        &page,
        options.inter_panel_crop,
        Background::White,
    );

    let effective_gamma = options
        .gamma
        .filter(|&g| g >= 0.1)
        .unwrap_or(options.profile.gamma);
    let page = crate::contrast::gamma_correct(&page, effective_gamma);

    let page = if options.noautocontrast {
        page
    } else {
        crate::contrast::autocontrast(&page, options.autolevel)
    };

    let resize_options = ResizeOptions {
        target,
        upscale: options.upscale,
        stretch: options.stretch,
        wallpaper: options.wallpaper,
        is_kdx_profile: options.profile.code == "KDX",
        pads_for_cbz_or_pdf: matches!(options.output_format, OutputFormat::Cbz | OutputFormat::Pdf),
        white_borders: options.white_borders,
        // fillCheck() (page background detection) isn't ported yet -- white
        // is the overwhelmingly common case for manga pages in the
        // meantime (see crate::crop::Background's doc comment).
        fill: 255,
    };
    let page = resize::resize_page(&page, &resize_options);

    let mut bytes = Vec::new();
    let extension = if options.force_png {
        let quantized =
            crate::quantize::quantize_with_floyd_steinberg(&page, options.profile.palette);
        image::DynamicImage::ImageLuma8(quantized)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)?;
        "png"
    } else {
        image::DynamicImage::ImageLuma8(page)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Jpeg)?;
        "jpg"
    };
    Ok((extension.to_string(), bytes))
}

fn crop_policy(options: &PipelineOptions) -> CropPolicy {
    CropPolicy {
        power: options.cropping_power,
        minimum_area_ratio: options.cropping_minimum as f64,
        preserve_margin_percent: 0.0,
        // fillCheck() isn't ported yet -- see process_page's fill comment.
        background: Background::White,
    }
}
