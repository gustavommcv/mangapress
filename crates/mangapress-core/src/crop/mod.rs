//! Crop algorithms. KCC keeps these as three cooperating modules sharing a
//! common thresholding primitive; we mirror that split rather than merging
//! them, since each is independently testable against its own fixtures.

pub mod inter_panel;
pub mod margin;
pub mod page_number;

use image::{GrayImage, Luma};

/// `threshold_from_power(power)` from `common_crop.py`: `240 - power * 64`.
/// Shared by [`margin`] and [`page_number`] — both grayscale, invert if the
/// page background is dark, autocontrast, box-blur, then threshold at this
/// value and take a bounding box of what's left.
pub fn threshold_from_power(power: f32) -> f32 {
    240.0 - power * 64.0
}

/// The page's detected background color (upstream: `fillCheck()` in
/// `image.py`, not yet ported — background color detection is out of scope
/// for the crop algorithms themselves, which just take it as an input).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    White,
    Dark,
}

/// A PIL-style bounding box: half-open, `right`/`bottom` exclusive
/// (`[left, right) x [top, bottom)`), matching `Image.getbbox()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bbox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// The final crop rectangle to actually apply, in absolute pixel
/// coordinates (half-open, like [`Bbox`]) — distinct from `Bbox` because by
/// the time we have one of these, [`CropPolicy`]'s 10% cap and
/// `--preservemargin` back-off have already been folded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropBox {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// Shared policy wrapped around either crop algorithm's raw bbox detection —
/// `cropMargin()`/`cropPageNumber()` + `maybeCrop()` in `image.py` are the
/// same wrapper applied to two different detectors.
#[derive(Debug, Clone, Copy)]
pub struct CropPolicy {
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

impl Default for CropPolicy {
    fn default() -> Self {
        CropPolicy {
            power: 1.0,
            minimum_area_ratio: 0.0,
            preserve_margin_percent: 0.0,
            background: Background::White,
        }
    }
}

/// `maybeCrop()`'s policy on top of a raw detected [`Bbox`]: cap to at most
/// 10% cropped per side, back off by `--preservemargin`, and only actually
/// crop if the result keeps at least `minimum_area_ratio` of the page.
/// Returns `None` when the computed crop doesn't clear the minimum-area
/// gate — meaning "leave the page as-is."
pub fn apply_policy(bbox: Bbox, image_size: (u32, u32), policy: &CropPolicy) -> Option<CropBox> {
    let capped = cap_to_ten_percent(bbox, image_size);
    let preserved = apply_preserve_margin(capped, image_size, policy.preserve_margin_percent);

    if area_ratio(preserved, image_size) >= policy.minimum_area_ratio {
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

/// `Image.getbbox()`: the bounding box of non-zero pixels, or `None` if the
/// image is entirely zero.
pub fn get_bbox(img: &GrayImage) -> Option<Bbox> {
    let (w, h) = img.dimensions();
    let mut min_x = None;
    let mut max_x = None;
    let mut min_y = None;
    let mut max_y = None;

    for y in 0..h {
        for x in 0..w {
            if img.get_pixel(x, y)[0] != 0 {
                min_x = Some(min_x.map_or(x, |v: u32| v.min(x)));
                max_x = Some(max_x.map_or(x, |v: u32| v.max(x)));
                min_y = Some(min_y.map_or(y, |v: u32| v.min(y)));
                max_y = Some(max_y.map_or(y, |v: u32| v.max(y)));
            }
        }
    }

    match (min_x, max_x, min_y, max_y) {
        (Some(l), Some(r), Some(t), Some(b)) => Some(Bbox {
            left: l,
            top: t,
            right: r + 1,
            bottom: b + 1,
        }),
        _ => None,
    }
}

/// `ignore_pixels_near_edge()` from `page_number_crop_alg.py`: clears each
/// of the four 2%-wide/tall edge strips of a binary (0/255) image if the
/// strip's fraction of foreground (255) pixels is low but nonzero — a
/// low-density scatter near the border reads as scan noise/dust, not real
/// content, and gets erased before the final bbox is computed. A strip with
/// *no* foreground pixels is already fine as-is; one with a high fraction is
/// assumed to be real content and is deliberately left alone.
pub fn ignore_pixels_near_edge(bw_img: &mut GrayImage) {
    let (w, h) = bw_img.dimensions();
    let edge_boxes = [
        (0, 0, w, (0.02 * h as f64) as u32),
        (0, (0.98 * h as f64) as u32, w, h),
        (0, 0, (0.02 * w as f64) as u32, h),
        ((0.98 * w as f64) as u32, 0, w, h),
    ];

    for (x0, y0, x1, y1) in edge_boxes {
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let area = (x1 - x0) as f64 * (y1 - y0) as f64;
        let mut foreground = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                if bw_img.get_pixel(x, y)[0] == 255 {
                    foreground += 1;
                }
            }
        }
        let imperfections = foreground as f64 / area;
        if imperfections > 0.0 && imperfections < 0.02 {
            for y in y0..y1 {
                for x in x0..x1 {
                    bw_img.put_pixel(x, y, Luma([0]));
                }
            }
        }
    }
}

/// Output of the shared prefix both [`margin::get_bbox_crop_margin`] and
/// [`page_number::get_bbox_crop_margin_page_number`] start from: grayscale
/// input, invert if the background is dark, autocontrast (cutoff 1%),
/// box-blur (radius 1), threshold, clear edge noise, take a bbox.
pub struct Binarized {
    /// The contrasted + blurred grayscale image (not yet thresholded) —
    /// [`page_number`]'s windowed digit-shape scan re-thresholds directly
    /// against this, matching upstream re-using the same intermediate
    /// image rather than the final binary mask.
    pub grayscale: GrayImage,
    pub binary: GrayImage,
    pub bbox: Option<Bbox>,
    pub threshold: f32,
}

pub fn binarize_for_crop(img: &GrayImage, power: f32, background: Background) -> Binarized {
    let prepped = match background {
        Background::White => img.clone(),
        Background::Dark => {
            let mut inverted = img.clone();
            image::imageops::invert(&mut inverted);
            inverted
        }
    };

    let grayscale = box_blur_radius1(&crate::contrast::autocontrast_cutoff(&prepped, 1));
    let threshold = threshold_from_power(power);
    let mut binary = threshold_binary(&grayscale, threshold);
    ignore_pixels_near_edge(&mut binary);
    let bbox = get_bbox(&binary);

    Binarized {
        grayscale,
        binary,
        bbox,
        threshold,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_power_matches_kcc_default() {
        // KCC's --croppingpower default is 1.0 -> threshold 176.
        assert_eq!(threshold_from_power(1.0), 176.0);
    }

    #[test]
    fn get_bbox_of_blank_image_is_none() {
        let img = GrayImage::from_pixel(20, 20, Luma([0]));
        assert_eq!(get_bbox(&img), None);
    }

    #[test]
    fn get_bbox_finds_tight_rectangle() {
        let mut img = GrayImage::from_pixel(20, 20, Luma([0]));
        for y in 5..10 {
            for x in 3..8 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        assert_eq!(
            get_bbox(&img),
            Some(Bbox {
                left: 3,
                top: 5,
                right: 8,
                bottom: 10
            })
        );
    }

    #[test]
    fn ignore_pixels_near_edge_clears_sparse_border_noise() {
        let mut img = GrayImage::from_pixel(100, 100, Luma([0]));
        // A single stray foreground pixel in the top 2% strip (density well
        // under 2%) — should be treated as scan noise and cleared.
        img.put_pixel(50, 0, Luma([255]));
        ignore_pixels_near_edge(&mut img);
        assert_eq!(img.get_pixel(50, 0)[0], 0);
    }

    #[test]
    fn ignore_pixels_near_edge_keeps_dense_border_content() {
        let mut img = GrayImage::from_pixel(100, 100, Luma([0]));
        // Fill the whole top strip (2 rows) — density 100%, well over the
        // 2% noise threshold, so this is assumed to be real content.
        for y in 0..2 {
            for x in 0..100 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        ignore_pixels_near_edge(&mut img);
        assert_eq!(img.get_pixel(50, 0)[0], 255);
    }

    #[test]
    fn large_margin_is_capped_to_ten_percent() {
        let bbox = Bbox {
            left: 60,
            top: 90,
            right: 140,
            bottom: 210,
        };
        let crop = apply_policy(bbox, (200, 300), &CropPolicy::default())
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
        let bbox = Bbox {
            left: 60,
            top: 90,
            right: 140,
            bottom: 210,
        };
        // The capped crop keeps (180-20)*(270-30)=38400 of 60000 px = 64%.
        let crop = apply_policy(
            bbox,
            (200, 300),
            &CropPolicy {
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
}
