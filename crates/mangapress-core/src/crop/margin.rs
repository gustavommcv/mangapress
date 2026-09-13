//! Whitespace margin cropping.
//!
//! Port target: `get_bbox_crop_margin()` (bbox detection, in
//! `page_number_crop_alg.py`) plus `cropMargin()`/`maybeCrop()` (the policy
//! wrapped around it, in `image.py` — see [`super::apply_policy`]).
//! `image.py` is GPLv3 — this is a reimplementation from its documented
//! behavior, not a port of its code (see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`); the bbox-detection
//! algorithm itself carries no license header upstream, so it's treated
//! with the same caution.

use super::{apply_policy, Background, Bbox, CropBox, CropPolicy};
use image::GrayImage;

/// `get_bbox_crop_margin()`: grayscale input assumed (the pipeline already
/// works in [`GrayImage`] by this stage), invert if the page background is
/// dark, autocontrast (cutoff 1%), box-blur (radius 1) to suppress scan
/// noise, threshold, clear sparse edge-noise, then take the bounding box of
/// what's left.
pub fn get_bbox_crop_margin(img: &GrayImage, power: f32, background: Background) -> Option<Bbox> {
    super::binarize_for_crop(img, power, background, true).bbox
}

/// `cropMargin()` + `maybeCrop()`.
pub fn compute_margin_crop(img: &GrayImage, policy: &CropPolicy) -> Option<CropBox> {
    let bbox = get_bbox_crop_margin(img, policy.power, policy.background)?;
    apply_policy(bbox, img.dimensions(), policy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

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
        let crop =
            compute_margin_crop(&img, &CropPolicy::default()).expect("a crop should be produced");
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
        let crop = compute_margin_crop(
            &img,
            &CropPolicy {
                minimum_area_ratio: 0.9,
                ..Default::default()
            },
        );
        assert_eq!(crop, None);
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
            &CropPolicy {
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
