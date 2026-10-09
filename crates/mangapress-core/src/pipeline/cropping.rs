use super::{CroppingMode, PipelineOptions};
use crate::crop::{self, Background, CropPolicy};

/// Margin (and page-number) cropping, then inter-panel cropping, on the
/// whole source page. `proxy` is the page's grayscale, which both detectors
/// read; the page's own RGB pixels are what gets cropped.
pub(super) fn crop_whole_page(
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

pub(super) fn crop_policy(options: &PipelineOptions, background: Background) -> CropPolicy {
    CropPolicy {
        power: options.cropping_power,
        minimum_area_ratio: (options.cropping_minimum as f64) / 100.0,
        preserve_margin_percent: options.preserve_margin_percent,
        background,
    }
}
