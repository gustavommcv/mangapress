//! Double-page spread detection: split vs. rotate vs. leave alone.
//!
//! Port target: `ComicPageParser.splitCheck()` in KCC's `image.py` (GPLv3
//! upstream — spec only, see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! Decision tree, given source `(width, height)` and device `(dst_width,
//! dst_height)`:
//! 1. Not a spread at all (`Decision::Normal`) unless *both*:
//!    - orientation mismatches the device (`(width > height) != (dst_width >
//!      dst_height)`), and
//!    - `width / height > SPREAD_ASPECT_THRESHOLD` (1.16).
//! 2. If it is a spread: `width / height >= BISECT_THRESHOLD` (1.8) means
//!    "too wide to usefully split" -> rotate instead, even in default Split
//!    mode. Below 1.8, respect the user's `-r/--splitter` choice (Split /
//!    Rotate / Both).
//! 3. Which half becomes "page one" when splitting depends on reading
//!    direction — see [`crate::manga`].
//!
//! Webtoon mode bypasses this entirely (pages pass through as `Normal`).

pub const SPREAD_ASPECT_THRESHOLD: f64 = 1.16;
pub const BISECT_THRESHOLD: f64 = 1.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Not a spread — process as a single ordinary page.
    Normal,
    /// Split into left/right (or top/bottom) halves.
    Split,
    /// Rotate the whole spread 90 degrees instead of splitting.
    Rotate,
    /// Produce both a split and a rotated version (`--splitter 2`).
    Both,
}

pub fn decide(
    src: (u32, u32),
    dst: (u32, u32),
    splitter: crate::pipeline::SplitterMode,
) -> Decision {
    let (w, h) = (src.0 as f64, src.1 as f64);
    let src_landscape = src.0 > src.1;
    let dst_landscape = dst.0 > dst.1;
    let is_spread = (src_landscape != dst_landscape) && (w / h > SPREAD_ASPECT_THRESHOLD);

    if !is_spread {
        return Decision::Normal;
    }

    let too_wide_to_split = w / h >= BISECT_THRESHOLD;
    use crate::pipeline::SplitterMode::*;
    match (too_wide_to_split, splitter) {
        (true, _) => Decision::Rotate,
        (false, Split) => Decision::Split,
        (false, Rotate) => Decision::Rotate,
        (false, Both) => Decision::Both,
    }
}

/// Executes a [`Decision`] against the actual page, producing the ordered
/// list of output pages it implies — this is `splitCheck()`'s payload
/// construction, the part that actually crops/rotates pixels rather than
/// just deciding to.
pub fn execute(
    img: &image::GrayImage,
    decision: Decision,
    manga_style: bool,
    rotate_right: bool,
) -> Vec<image::GrayImage> {
    match decision {
        Decision::Normal => vec![img.clone()],
        Decision::Split => {
            let (page_one, page_two) = split(img, manga_style);
            vec![page_one, page_two]
        }
        Decision::Rotate => vec![rotate(img, rotate_right)],
        Decision::Both => {
            let (page_one, page_two) = split(img, manga_style);
            vec![page_one, page_two, rotate(img, rotate_right)]
        }
    }
}

/// Splits a spread down the middle: vertically (left/right) if the source
/// is landscape, horizontally (top/bottom) if portrait — matching
/// `splitCheck()`'s `leftbox`/`rightbox` geometry exactly. Order respects
/// reading direction: right-to-left reads the second (right or bottom)
/// half first.
fn split(img: &image::GrayImage, manga_style: bool) -> (image::GrayImage, image::GrayImage) {
    let (w, h) = img.dimensions();
    let (first_box, second_box) = if w > h {
        ((0, 0, w / 2, h), (w / 2, 0, w - w / 2, h))
    } else {
        ((0, 0, w, h / 2), (0, h / 2, w, h - h / 2))
    };

    let crop =
        |b: (u32, u32, u32, u32)| image::imageops::crop_imm(img, b.0, b.1, b.2, b.3).to_image();
    let (first, second) = (crop(first_box), crop(second_box));

    if manga_style {
        (second, first)
    } else {
        (first, second)
    }
}

/// Rotates a whole spread 90 degrees so it fills the device's orientation.
/// Default direction is counter-clockwise, matching upstream's
/// `image.rotate(90, ...)` (PIL rotates counter-clockwise for positive
/// angles); `rotate_right` matches `--rotateright`'s `rotate(-90, ...)`.
fn rotate(img: &image::GrayImage, rotate_right: bool) -> image::GrayImage {
    if rotate_right {
        image::imageops::rotate90(img)
    } else {
        image::imageops::rotate270(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::SplitterMode;

    const PORTRAIT_DEVICE: (u32, u32) = (1072, 1448);

    #[test]
    fn narrow_landscape_page_is_not_a_spread() {
        // 1.16 threshold not crossed.
        assert_eq!(
            decide((1200, 1100), PORTRAIT_DEVICE, SplitterMode::Split),
            Decision::Normal
        );
    }

    #[test]
    fn typical_two_page_spread_splits_by_default() {
        assert_eq!(
            decide((2000, 1448), PORTRAIT_DEVICE, SplitterMode::Split),
            Decision::Split
        );
    }

    #[test]
    fn very_wide_spread_rotates_even_in_split_mode() {
        assert_eq!(
            decide((3000, 1448), PORTRAIT_DEVICE, SplitterMode::Split),
            Decision::Rotate
        );
    }

    #[test]
    fn portrait_page_on_portrait_device_is_never_a_spread() {
        assert_eq!(
            decide((1072, 1448), PORTRAIT_DEVICE, SplitterMode::Split),
            Decision::Normal
        );
    }

    fn landscape_page_with_left_right_halves() -> image::GrayImage {
        // 200x100: left half filled with 10, right half filled with 200.
        image::GrayImage::from_fn(200, 100, |x, _y| {
            image::Luma([if x < 100 { 10 } else { 200 }])
        })
    }

    #[test]
    fn split_of_landscape_image_is_vertical() {
        let img = landscape_page_with_left_right_halves();
        let (first, second) = split(&img, false);
        assert_eq!(first.dimensions(), (100, 100));
        assert_eq!(second.dimensions(), (100, 100));
        assert_eq!(first.get_pixel(0, 0)[0], 10);
        assert_eq!(second.get_pixel(0, 0)[0], 200);
    }

    #[test]
    fn split_order_flips_for_manga_style() {
        let img = landscape_page_with_left_right_halves();
        let (first, _second) = split(&img, true);
        // Right-to-left: the right half (200) comes first.
        assert_eq!(first.get_pixel(0, 0)[0], 200);
    }

    #[test]
    fn split_of_portrait_image_is_horizontal() {
        // 100x200, top half 10, bottom half 200.
        let img = image::GrayImage::from_fn(100, 200, |_x, y| {
            image::Luma([if y < 100 { 10 } else { 200 }])
        });
        let (first, second) = split(&img, false);
        assert_eq!(first.dimensions(), (100, 100));
        assert_eq!(first.get_pixel(0, 0)[0], 10);
        assert_eq!(second.get_pixel(0, 0)[0], 200);
    }

    #[test]
    fn rotate_swaps_dimensions() {
        let img = image::GrayImage::from_pixel(200, 100, image::Luma([0]));
        assert_eq!(rotate(&img, false).dimensions(), (100, 200));
        assert_eq!(rotate(&img, true).dimensions(), (100, 200));
    }

    #[test]
    fn rotate_direction_differs_between_default_and_rotateright() {
        // A single distinctive corner pixel lands in a different place
        // depending on rotation direction (checking the whole image rather
        // than one fixed coordinate, since that coordinate could coincide
        // with 0 in both outputs without the images being the same).
        let mut img = image::GrayImage::from_pixel(4, 2, image::Luma([0]));
        img.put_pixel(0, 0, image::Luma([255]));
        let default_rotated = rotate(&img, false);
        let right_rotated = rotate(&img, true);
        assert_ne!(default_rotated, right_rotated);
    }

    #[test]
    fn execute_normal_produces_one_unchanged_page() {
        let img = image::GrayImage::from_pixel(10, 10, image::Luma([42]));
        let out = execute(&img, Decision::Normal, false, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].dimensions(), (10, 10));
    }

    #[test]
    fn execute_split_produces_two_pages() {
        let img = landscape_page_with_left_right_halves();
        let out = execute(&img, Decision::Split, false, false);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn execute_rotate_produces_one_rotated_page() {
        let img = image::GrayImage::from_pixel(200, 100, image::Luma([0]));
        let out = execute(&img, Decision::Rotate, false, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].dimensions(), (100, 200));
    }

    #[test]
    fn execute_both_produces_three_pages() {
        let img = landscape_page_with_left_right_halves();
        let out = execute(&img, Decision::Both, false, false);
        assert_eq!(out.len(), 3);
    }
}
