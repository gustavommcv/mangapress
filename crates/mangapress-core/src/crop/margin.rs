//! Whitespace margin cropping.
//!
//! Port target: `get_bbox_crop_margin()` (bbox detection) and
//! `cropMargin()`/`maybeCrop()` (the policy wrapped around it — 10% cap,
//! `--preservemargin` back-off, `--croppingminimum` gate) in KCC's
//! `page_number_crop_alg.py`/`image.py`. `image.py` is GPLv3 — this is a
//! reimplementation from its documented behavior, not a port of its code
//! (see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`); the bbox-detection
//! algorithm itself (`page_number_crop_alg.py`) carries no license header
//! upstream, so it's treated with the same caution.

use super::{get_bbox, ignore_pixels_near_edge, Background, Bbox};
use crate::contrast::autocontrast_cutoff;
use image::{GrayImage, Luma};

/// The final crop rectangle to actually apply, in absolute pixel
/// coordinates (half-open, like [`Bbox`]) — distinct from `Bbox` because by
/// the time we have one of these, the 10% cap and `--preservemargin`
/// back-off have already been folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropBox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct MarginCropOptions {
    /// `-c/--croppingpower`. Higher power crops through more.
    pub power: f32,
    /// `--cm/--croppingminimum`: only actually crop if the crop region
    /// would keep at least this fraction of the page's area.
    pub minimum_area_ratio: f64,
    /// `--preservemargin`: back the computed crop off by this percentage
    /// after the 10% cap, so *some* margin is deliberately kept.
    pub preserve_margin_percent: f32,
    pub background: Background,
}

impl Default for MarginCropOptions {
    fn default() -> Self {
        MarginCropOptions {
            power: 1.0,
            minimum_area_ratio: 0.0,
            preserve_margin_percent: 0.0,
            background: Background::White,
        }
    }
}

/// `get_bbox_crop_margin()`: grayscale input assumed (the pipeline already
/// works in [`GrayImage`] by this stage), invert if the page background is
/// dark, autocontrast (cutoff 1%), box-blur (radius 1) to suppress scan
/// noise, threshold at [`super::threshold_from_power`], clear sparse
/// edge-noise, then take the bounding box of what's left.
pub fn get_bbox_crop_margin(img: &GrayImage, power: f32, background: Background) -> Option<Bbox> {
    let prepped = match background {
        Background::White => img.clone(),
        Background::Dark => {
            let mut inverted = img.clone();
            image::imageops::invert(&mut inverted);
            inverted
        }
    };

    let contrasted = autocontrast_cutoff(&prepped, 1);
    let blurred = box_blur_radius1(&contrasted);
    let threshold = super::threshold_from_power(power);
    let mut bw = threshold_binary(&blurred, threshold);
    ignore_pixels_near_edge(&mut bw);
    get_bbox(&bw)
}

/// `cropMargin()` + `maybeCrop()`: the full policy on top of the raw bbox —
/// cap to at most 10% cropped per side, back off by `--preservemargin`, and
/// only actually crop if the result keeps at least `minimum_area_ratio` of
/// the page. Returns `None` when either no content-vs-margin boundary was
/// found at all, or the computed crop doesn't clear the minimum-area gate —
/// both cases mean "leave the page as-is."
pub fn compute_margin_crop(img: &GrayImage, options: &MarginCropOptions) -> Option<CropBox> {
    let bbox = get_bbox_crop_margin(img, options.power, options.background)?;
    let (w, h) = img.dimensions();

    let capped = cap_to_ten_percent(bbox, (w, h));
    let preserved = apply_preserve_margin(capped, (w, h), options.preserve_margin_percent);

    if area_ratio(preserved, (w, h)) >= options.minimum_area_ratio {
        Some(preserved)
    } else {
        None
    }
}

/// Crop an image to a previously computed [`CropBox`].
pub fn apply_crop(img: &GrayImage, crop: CropBox) -> GrayImage {
    image::imageops::crop_imm(
        img,
        crop.left,
        crop.top,
        crop.right - crop.left,
        crop.bottom - crop.top,
    )
    .to_image()
}

fn box_blur_radius1(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    GrayImage::from_fn(w, h, |x, y| {
        let mut sum: u32 = 0;
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as u32;
                let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as u32;
                sum += img.get_pixel(sx, sy)[0] as u32;
            }
        }
        Luma([(sum / 9) as u8])
    })
}

fn threshold_binary(img: &GrayImage, threshold: f32) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y)[0] as f32;
        if p <= threshold {
            Luma([255])
        } else {
            Luma([0])
        }
    })
}

/// `bbox = (min(0.1*w, left), min(0.1*h, upper), max(0.9*w, right),
/// max(0.9*h, lower))` — caps how much can be cropped off each side to at
/// most 10% of that dimension, without forcing a full 10% crop when the
/// detected margin is smaller than that.
fn cap_to_ten_percent(bbox: Bbox, (w, h): (u32, u32)) -> CropBox {
    let (w, h) = (w as f64, h as f64);
    CropBox {
        left: (0.1 * w).min(bbox.left as f64).round() as u32,
        top: (0.1 * h).min(bbox.top as f64).round() as u32,
        right: (0.9 * w).max(bbox.right as f64).round() as u32,
        bottom: (0.9 * h).max(bbox.bottom as f64).round() as u32,
    }
}

/// `ratio = 1 - preservemargin/100; box = left*ratio, upper*ratio,
/// right+(w-right)*(1-ratio), lower+(h-lower)*(1-ratio)`. With
/// `preserve_margin_percent == 0` this is an identity transform (`ratio ==
/// 1`), so upstream's `if self.opt.preservemargin:` guard doesn't need a
/// separate branch here.
fn apply_preserve_margin(
    crop: CropBox,
    (w, h): (u32, u32),
    preserve_margin_percent: f32,
) -> CropBox {
    let ratio = 1.0 - (preserve_margin_percent as f64) / 100.0;
    let (w, h) = (w as f64, h as f64);
    CropBox {
        left: (crop.left as f64 * ratio).round() as u32,
        top: (crop.top as f64 * ratio).round() as u32,
        right: (crop.right as f64 + (w - crop.right as f64) * (1.0 - ratio)).round() as u32,
        bottom: (crop.bottom as f64 + (h - crop.bottom as f64) * (1.0 - ratio)).round() as u32,
    }
}

fn area_ratio(crop: CropBox, (w, h): (u32, u32)) -> f64 {
    let box_area =
        crop.right.saturating_sub(crop.left) as f64 * crop.bottom.saturating_sub(crop.top) as f64;
    let image_area = w as f64 * h as f64;
    box_area / image_area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white_page_with_black_rect(
        size: (u32, u32),
        rect: (u32, u32, u32, u32), // left, top, right, bottom (exclusive)
    ) -> GrayImage {
        let (w, h) = size;
        GrayImage::from_fn(w, h, |x, y| {
            let (l, t, r, b) = rect;
            if x >= l && x < r && y >= t && y < b {
                Luma([0])
            } else {
                Luma([255])
            }
        })
    }

    #[test]
    fn blank_white_page_has_no_bbox() {
        let img = GrayImage::from_pixel(200, 300, Luma([255]));
        assert_eq!(get_bbox_crop_margin(&img, 1.0, Background::White), None);
    }

    #[test]
    fn small_margin_passes_through_close_to_detected() {
        // 5px margin on all sides of a 200x300 page (2.5%/1.7%) — well
        // under the 10% cap, so the final crop should track the detected
        // content closely (allow a couple pixels of blur/threshold slop).
        let img = white_page_with_black_rect((200, 300), (5, 5, 195, 295));
        let bbox =
            get_bbox_crop_margin(&img, 1.0, Background::White).expect("content should be detected");
        assert!(bbox.left <= 7, "left={}", bbox.left);
        assert!(bbox.top <= 7, "top={}", bbox.top);
        assert!(bbox.right >= 193, "right={}", bbox.right);
        assert!(bbox.bottom >= 293, "bottom={}", bbox.bottom);
    }

    #[test]
    fn large_margin_is_capped_to_ten_percent() {
        // 60/90px margins (30%) on a 200x300 page — the cap should clamp
        // the final crop to exactly the 10%/90% lines regardless of exact
        // blur-induced edge jitter in the raw detected bbox.
        let img = white_page_with_black_rect((200, 300), (60, 90, 140, 210));
        let crop = compute_margin_crop(
            &img,
            &MarginCropOptions {
                power: 1.0,
                ..Default::default()
            },
        )
        .expect("a crop should be produced");
        assert_eq!(
            crop,
            CropBox {
                left: 20,
                top: 30,
                right: 180,
                bottom: 270
            }
        );
    }

    #[test]
    fn minimum_area_ratio_suppresses_crop_when_too_aggressive() {
        let img = white_page_with_black_rect((200, 300), (60, 90, 140, 210));
        // The capped crop keeps (180-20)*(270-30)=38400 of 60000 px = 64%.
        // A minimum above that should suppress the crop entirely.
        let crop = compute_margin_crop(
            &img,
            &MarginCropOptions {
                power: 1.0,
                minimum_area_ratio: 0.9,
                ..Default::default()
            },
        );
        assert_eq!(crop, None);
    }

    #[test]
    fn preserve_margin_backs_off_the_crop() {
        let full = CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270,
        };
        // 50% preserve margin should move each edge halfway back to the
        // image border.
        let backed_off = apply_preserve_margin(full, (200, 300), 50.0);
        assert_eq!(
            backed_off,
            CropBox {
                left: 10,
                top: 15,
                right: 190,
                bottom: 285
            }
        );
    }

    #[test]
    fn preserve_margin_zero_is_identity() {
        let full = CropBox {
            left: 20,
            top: 30,
            right: 180,
            bottom: 270,
        };
        assert_eq!(apply_preserve_margin(full, (200, 300), 0.0), full);
    }

    #[test]
    fn apply_crop_produces_expected_dimensions() {
        let img = GrayImage::from_pixel(200, 300, Luma([255]));
        let cropped = apply_crop(
            &img,
            CropBox {
                left: 20,
                top: 30,
                right: 180,
                bottom: 270,
            },
        );
        assert_eq!(cropped.dimensions(), (160, 240));
    }

    #[test]
    fn dark_background_page_is_inverted_before_thresholding() {
        // A black page with a white rectangle of "content" — same shape as
        // the white-background case, just polarity-flipped end to end.
        let (w, h) = (200u32, 300u32);
        let img = GrayImage::from_fn(w, h, |x, y| {
            if (60..140).contains(&x) && (90..210).contains(&y) {
                Luma([255])
            } else {
                Luma([0])
            }
        });
        let crop = compute_margin_crop(
            &img,
            &MarginCropOptions {
                power: 1.0,
                background: Background::Dark,
                ..Default::default()
            },
        )
        .expect("a crop should be produced");
        assert_eq!(
            crop,
            CropBox {
                left: 20,
                top: 30,
                right: 180,
                bottom: 270
            }
        );
    }
}
