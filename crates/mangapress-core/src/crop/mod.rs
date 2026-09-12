//! Crop algorithms. KCC keeps these as three cooperating modules sharing a
//! common thresholding primitive; we mirror that split rather than merging
//! them, since each is independently testable against its own fixtures.

pub mod inter_panel;
pub mod margin;
pub mod page_number;

use image::GrayImage;

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
                    bw_img.put_pixel(x, y, image::Luma([0]));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

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
}
