//! Double-page spread detection: split vs. rotate vs. leave alone.
//!
//! Upstream reference: `ComicPageParser.splitCheck()` in KCC's `image.py` (GPLv3
//! upstream — spec only, see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! Decision tree, given the page's `(width, height)` *after* margin cropping
//! (upstream crops before it looks for a spread, since KCC 12.0.0 — see
//! [`crate::pipeline::process_page`]) and device `(dst_width, dst_height)`:
//! 0. In `Rotate` mode only: a page whose orientation mismatches the device
//!    and which already fits the device once turned (`width <= dst_height`
//!    and `height <= dst_width`) is rotated whatever its aspect ratio —
//!    upstream tests this before the aspect threshold below, so a landscape
//!    page too close to square to count as a spread is still turned to fill
//!    the screen. Confirmed against real KCC 12.0.0: a 1000x900 page on a
//!    1072x1448 device comes out rotated (1072x1191), not upright (1072x965).
//! 1. Not a spread at all (`Decision::Normal`) unless *both*:
//!    - orientation mismatches the device (`(width > height) != (dst_width >
//!      dst_height)`), and
//!    - `width / height > SPREAD_ASPECT_THRESHOLD` (1.16).
//! 2. If it is a spread: `width / height >= ROTATE_ONLY_ASPECT_THRESHOLD`
//!    (1.8) means "too wide to usefully split" -> rotate instead, even in
//!    default Split mode. Below 1.8, respect the user's `-r/--splitter`
//!    choice (Split / Rotate / Both).
//! 3. Which half becomes "page one" when splitting depends on reading
//!    direction — see [`crate::manga`].
//!
//! Webtoon mode bypasses this entirely (pages pass through as `Normal`).

pub const SPREAD_ASPECT_THRESHOLD: f64 = 1.16;
pub const ROTATE_ONLY_ASPECT_THRESHOLD: f64 = 1.8;

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
    let orientation_mismatch = src_landscape != dst_landscape;
    use crate::pipeline::SplitterMode::*;

    if orientation_mismatch && splitter == Rotate && src.0 <= dst.1 && src.1 <= dst.0 {
        return Decision::Rotate;
    }

    let is_spread = orientation_mismatch && (w / h > SPREAD_ASPECT_THRESHOLD);

    if !is_spread {
        return Decision::Normal;
    }

    let too_wide_to_split = w / h >= ROTATE_ONLY_ASPECT_THRESHOLD;
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
/// What an output page is, relative to the source page it came from —
/// upstream's `-kcc-x` / `-kcc-b` / `-kcc-c` / `-kcc-a|d` file-name suffixes.
/// The EPUB builder needs it to place pages on the right side of a two-page
/// view (see [`crate::ebook::epub`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageRole {
    /// An ordinary page, not part of a spread.
    #[default]
    Normal,
    /// The half of a split spread that is read first.
    SplitFirst,
    /// The half of a split spread that is read second.
    SplitSecond,
    /// A whole spread, rotated to fill the screen.
    Rotated,
}

/// The [`PageRole`] of each page [`execute`] returns for `decision`, in the
/// same order.
pub fn roles(decision: Decision) -> &'static [PageRole] {
    match decision {
        Decision::Normal => &[PageRole::Normal],
        Decision::Split => &[PageRole::SplitFirst, PageRole::SplitSecond],
        Decision::Rotate => &[PageRole::Rotated],
        Decision::Both => &[
            PageRole::SplitFirst,
            PageRole::SplitSecond,
            PageRole::Rotated,
        ],
    }
}

/// An owned image of any pixel type: spreads are split and rotated while the
/// page is still RGB (see [`crate::pipeline::process_page`]), and the tests
/// below exercise the same code on grayscale.
type Buffer<P> = image::ImageBuffer<P, Vec<<P as image::Pixel>::Subpixel>>;

pub fn execute<P: image::Pixel + 'static>(
    img: &Buffer<P>,
    decision: Decision,
    manga_style: bool,
    rotate_right: bool,
) -> Vec<Buffer<P>> {
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
fn split<P: image::Pixel + 'static>(img: &Buffer<P>, manga_style: bool) -> (Buffer<P>, Buffer<P>) {
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
fn rotate<P: image::Pixel + 'static>(img: &Buffer<P>, rotate_right: bool) -> Buffer<P> {
    if rotate_right {
        image::imageops::rotate90(img)
    } else {
        image::imageops::rotate270(img)
    }
}

/// `--maximizestrips`: the page's two halves, the first-read one on top, on
/// a canvas half as wide and twice as tall. When the width is odd the right
/// half is one column wider than the canvas and loses that column, as it
/// does upstream.
pub(super) fn stack_halves(page: &image::RgbImage, manga_style: bool) -> image::RgbImage {
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

    #[test]
    fn rotate_mode_turns_a_page_that_fits_once_turned_whatever_its_aspect() {
        // 1000x900 is 1.11:1 — under the 1.16 spread threshold — but it is
        // landscape on a portrait device and fits it rotated.
        assert_eq!(
            decide((1000, 900), PORTRAIT_DEVICE, SplitterMode::Rotate),
            Decision::Rotate
        );
        // The same page is left alone in the other two modes...
        assert_eq!(
            decide((1000, 900), PORTRAIT_DEVICE, SplitterMode::Split),
            Decision::Normal
        );
        assert_eq!(
            decide((1000, 900), PORTRAIT_DEVICE, SplitterMode::Both),
            Decision::Normal
        );
        // ...and so is one that would not fit the device once turned.
        assert_eq!(
            decide((1500, 1400), PORTRAIT_DEVICE, SplitterMode::Rotate),
            Decision::Normal
        );
    }

    #[test]
    fn roles_name_each_page_execute_returns_in_order() {
        let img = image::GrayImage::new(200, 100);
        for decision in [
            Decision::Normal,
            Decision::Split,
            Decision::Rotate,
            Decision::Both,
        ] {
            assert_eq!(
                roles(decision).len(),
                execute(&img, decision, true, false).len()
            );
        }
        assert_eq!(
            roles(Decision::Both),
            [
                PageRole::SplitFirst,
                PageRole::SplitSecond,
                PageRole::Rotated
            ]
        );
    }
}
