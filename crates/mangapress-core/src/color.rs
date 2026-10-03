//! What a source page's colors mean to the pipeline: how an RGB page
//! becomes gray, and whether upstream would call the page a color page.
//!
//! Port targets, both in KCC's `image.py` (GPLv3 upstream — reimplemented
//! from documented/observed behavior, not copied; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`):
//! - every `convert('L')` / `ImageOps.grayscale()` call (background
//!   detection, the crop algorithms' proxy image, the final
//!   `convertToGrayscale()`), i.e. Pillow's own RGB -> L conversion;
//! - `colorCheck()` / `calculate_color()`, the per-page heuristic deciding
//!   whether a page has meaningful color at all.
//!
//! A page is kept in color only with `--forcecolor`, and only if that
//! verdict calls it a color page. But the verdict also changes what happens
//! to a page that *ends up* gray, which is every page by default: a color
//! page is not autocontrasted (short of `--colorautocontrast`), and a color
//! first page — a cover — is never cropped. Both were missing here before
//! this module
//! existed: on a real 432-page black-and-white volume upstream flags 35
//! pages as color (covers, tinted scans), and those came out a mean 9 gray
//! levels (worst page: 18) away from upstream's because they were being
//! autocontrasted.
//!
//! Both conversions below were checked against real Pillow rather than
//! assumed: the gray formula on 4.5 million sampled colors, the chroma
//! formula on all 16,777,216 RGB triples.

use image::{GrayImage, RgbImage};

/// Pillow's RGB -> L: ITU-R 601-2 luma in 16-bit fixed point,
/// `(R*19595 + G*38470 + B*7471 + 0x8000) >> 16`. Not the `image` crate's
/// own `to_luma8()`, which weights the channels by Rec. 709
/// (0.2126/0.7152/0.0722) instead — identical on a neutral pixel, different
/// on a colored one, and enough on a tinted page to move its crop box: four
/// of the five pages whose output size disagreed with upstream's on a real
/// volume were color pages cropped from a Rec. 709 proxy.
pub fn to_gray(rgb: &RgbImage) -> GrayImage {
    let gray: Vec<u8> = rgb
        .as_raw()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| {
            ((p[0] as u32 * 19595 + p[1] as u32 * 38470 + p[2] as u32 * 7471 + 0x8000) >> 16) as u8
        })
        .collect();
    GrayImage::from_raw(rgb.width(), rgb.height(), gray)
        .expect("one gray sample per RGB pixel always fills the buffer exactly")
}

/// One of Pillow's RGB -> YCbCr lookup tables: `coefficient * 64 * i + 0.5`
/// truncated toward zero (a C integer cast, so a negative entry rounds
/// *toward* zero rather than to nearest), with the JFIF coefficients written
/// to five decimals. The six-decimal coefficients, and every other rounding
/// rule tried, fail to reproduce Pillow on some colors.
fn chroma_table(coefficient: f64) -> [i32; 256] {
    let mut table = [0i32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        *slot = (coefficient * 64.0 * i as f64 + 0.5) as i32;
    }
    table
}

/// Pillow's RGB <-> YCbCr conversion, both ways, as lookup tables. Needed
/// exactly, not approximately: upstream's color test reads the chroma
/// histograms, and its `--autolevel` on a color page sends every pixel
/// through YCbCr and back — a round trip that is lossy by a level or two in
/// Pillow's integer arithmetic, so "the same picture" is only the same if
/// the loss is too. Checked against Pillow over all 16,777,216 triples in
/// each direction.
pub struct YCbCr {
    y: [[i32; 256]; 3],
    cb: [[i32; 256]; 3],
    cr: [[i32; 256]; 3],
    r_from_cr: [i32; 256],
    g_from_cb: [i32; 256],
    g_from_cr: [i32; 256],
    b_from_cb: [i32; 256],
}

impl Default for YCbCr {
    fn default() -> Self {
        Self::new()
    }
}

impl YCbCr {
    pub fn new() -> Self {
        // The way back is tabulated around chroma's zero point, 128.
        let centered = |coefficient: f64| {
            let mut table = [0i32; 256];
            for (i, slot) in table.iter_mut().enumerate() {
                *slot = (coefficient * 64.0 * (i as f64 - 128.0) + 0.5) as i32;
            }
            table
        };
        YCbCr {
            y: [
                chroma_table(0.299),
                chroma_table(0.587),
                chroma_table(0.114),
            ],
            cb: [
                chroma_table(-0.16874),
                chroma_table(-0.33126),
                chroma_table(0.5),
            ],
            cr: [
                chroma_table(0.5),
                chroma_table(-0.41869),
                chroma_table(-0.08131),
            ],
            r_from_cr: centered(1.402),
            g_from_cb: centered(-0.34414),
            g_from_cr: centered(-0.71414),
            b_from_cb: centered(1.772),
        }
    }

    /// One RGB pixel as `[Y, Cb, Cr]`.
    pub fn from_rgb(&self, [r, g, b]: [u8; 3]) -> [u8; 3] {
        let (r, g, b) = (r as usize, g as usize, b as usize);
        // `>>` on a negative i32 floors, as C's does on the same sum.
        [
            ((self.y[0][r] + self.y[1][g] + self.y[2][b]) >> 6) as u8,
            (((self.cb[0][r] + self.cb[1][g] + self.cb[2][b]) >> 6) + 128) as u8,
            (((self.cr[0][r] + self.cr[1][g] + self.cr[2][b]) >> 6) + 128) as u8,
        ]
    }

    /// One `[Y, Cb, Cr]` pixel as RGB, each channel clamped to 0..=255.
    pub fn to_rgb(&self, [y, cb, cr]: [u8; 3]) -> [u8; 3] {
        let (y, cb, cr) = (y as i32, cb as usize, cr as usize);
        [
            (y + (self.r_from_cr[cr] >> 6)).clamp(0, 255) as u8,
            (y + ((self.g_from_cb[cb] + self.g_from_cr[cr]) >> 6)).clamp(0, 255) as u8,
            (y + (self.b_from_cb[cb] >> 6)).clamp(0, 255) as u8,
        ]
    }
}

/// The page's Cb and Cr histograms, as Pillow's `convert('YCbCr')` followed
/// by `histogram()` on each chroma band would give them.
fn chroma_histograms(rgb: &RgbImage) -> ([u64; 256], [u64; 256]) {
    let ycbcr = YCbCr::new();
    let mut cb = [0u64; 256];
    let mut cr = [0u64; 256];
    for &pixel in rgb.as_raw().as_chunks::<3>().0 {
        let [_, pixel_cb, pixel_cr] = ycbcr.from_rgb(pixel);
        cb[pixel_cb as usize] += 1;
        cr[pixel_cr as usize] += 1;
    }
    (cb, cr)
}

/// Removes `cut` pixels from the low end of a histogram, then `cut` from the
/// high end — the same count both times, taken from the page's total before
/// either end is touched.
fn trim_both_ends(histogram: &mut [u64; 256], cut: u64) {
    let mut remaining = cut;
    for bin in histogram.iter_mut() {
        if remaining == 0 {
            break;
        }
        let taken = remaining.min(*bin);
        *bin -= taken;
        remaining -= taken;
    }
    let mut remaining = cut;
    for bin in histogram.iter_mut().rev() {
        if remaining == 0 {
            break;
        }
        let taken = remaining.min(*bin);
        *bin -= taken;
        remaining -= taken;
    }
}

/// The lowest and highest chroma values still present in a histogram.
fn occupied_range(histogram: &[u64; 256]) -> Option<(i32, i32)> {
    let low = histogram.iter().position(|&count| count > 0)?;
    let high = histogram.iter().rposition(|&count| count > 0)?;
    Some((low as i32, high as i32))
}

/// How many pixels each pass trims off each end of the chroma histograms
/// before judging what is left.
enum Trim {
    Nothing,
    /// 0.2% of the page, as upstream's float floor division gives it.
    FifthOfAPercent,
    /// 3% of the page, in integer arithmetic.
    ThreePercent,
}

/// `calculate_color()` without `--forcecolor`: is there real color on this
/// page, as opposed to a gray page with JPEG chroma noise or a faint cast?
///
/// Neutral chroma is 128. Three passes look at how far the page's Cb and Cr
/// values stray from it, each pass trimming more of the outliers and asking
/// for less of what remains: untrimmed, a value 22 or more away; with 0.2%
/// trimmed off each end, 10 or more; with 3% trimmed, 4 or more. A pass
/// settles the question either way — gray when both chroma channels span
/// fewer than 7 values in total, color when any end reaches its distance —
/// and otherwise hands over to the next one; a page no pass settles is gray.
///
/// Only meaningful for a page decoded from a color image: upstream answers
/// "gray" for a single-channel source without looking, which the caller
/// does too (see `pipeline::process_page`).
pub fn has_meaningful_color(rgb: &RgbImage) -> bool {
    judge_color(rgb, false)
}

/// The same question when color output was asked for (`--forcecolor`), which
/// changes one rule: a narrow chroma spread no longer settles a page as
/// gray. Instead, a pass calls the page color as soon as either chroma
/// channel sits entirely on one side of neutral — so a page with an even
/// tint, which [`has_meaningful_color`] reads as a gray page with a cast, is
/// kept in color when color is what the reader wants.
pub fn has_color_worth_keeping(rgb: &RgbImage) -> bool {
    judge_color(rgb, true)
}

fn judge_color(rgb: &RgbImage, keeping_color: bool) -> bool {
    const NEUTRAL: i32 = 128;
    const NARROW_SPREAD: i32 = 7;

    let (cb_full, cr_full) = chroma_histograms(rgb);
    let pixels: u64 = cb_full.iter().sum();

    for (trim, distance) in [
        (Trim::Nothing, 22),
        (Trim::FifthOfAPercent, 10),
        (Trim::ThreePercent, 4),
    ] {
        let cut = match trim {
            Trim::Nothing => 0,
            Trim::FifthOfAPercent => (pixels as f64 * 0.2 / 100.0).floor() as u64,
            Trim::ThreePercent => pixels * 3 / 100,
        };
        let (mut cb, mut cr) = (cb_full, cr_full);
        trim_both_ends(&mut cb, cut);
        trim_both_ends(&mut cr, cut);
        let (Some((cb_low, cb_high)), Some((cr_low, cr_high))) =
            (occupied_range(&cb), occupied_range(&cr))
        else {
            return false;
        };

        if keeping_color {
            if cb_low > NEUTRAL || cr_low > NEUTRAL || cb_high < NEUTRAL || cr_high < NEUTRAL {
                return true;
            }
        } else if cb_high - cb_low < NARROW_SPREAD && cr_high - cr_low < NARROW_SPREAD {
            return false;
        }
        if cb_low <= NEUTRAL - distance
            || cr_low <= NEUTRAL - distance
            || cb_high >= NEUTRAL + distance
            || cr_high >= NEUTRAL + distance
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn a_neutral_pixel_keeps_its_value() {
        for v in [0u8, 1, 17, 128, 254, 255] {
            let img = RgbImage::from_pixel(1, 1, Rgb([v, v, v]));
            assert_eq!(to_gray(&img).get_pixel(0, 0)[0], v);
        }
    }

    #[test]
    fn primaries_are_weighted_as_pillow_weights_them() {
        // Pillow: Image.new('RGB', (1, 1), c).convert('L') for each color.
        for (color, expected) in [
            ([255u8, 0, 0], 76u8),
            ([0, 255, 0], 150),
            ([0, 0, 255], 29),
            ([200, 100, 50], 124),
        ] {
            let img = RgbImage::from_pixel(1, 1, Rgb(color));
            assert_eq!(to_gray(&img).get_pixel(0, 0)[0], expected, "{color:?}");
        }
    }

    #[test]
    fn chroma_tables_round_a_negative_entry_toward_zero() {
        // The first entries of Pillow's own Cb-from-R and Cr-from-G tables.
        assert_eq!(chroma_table(-0.16874)[..6], [0, -10, -21, -31, -42, -53]);
        assert_eq!(chroma_table(-0.41869)[..6], [0, -26, -53, -79, -106, -133]);
        assert_eq!(chroma_table(-0.16874)[255], -2753);
        assert_eq!(chroma_table(0.5)[255], 8160);
    }

    #[test]
    fn a_gray_page_has_neutral_chroma_and_no_color() {
        let img = RgbImage::from_fn(64, 64, |x, y| {
            let v = ((x * 4 + y) % 256) as u8;
            Rgb([v, v, v])
        });
        let (cb, cr) = chroma_histograms(&img);
        assert_eq!(cb[128], 64 * 64);
        assert_eq!(cr[128], 64 * 64);
        assert!(!has_meaningful_color(&img));
    }

    #[test]
    fn a_page_with_a_saturated_region_is_color() {
        let img = RgbImage::from_fn(64, 64, |x, _| {
            if x < 32 {
                Rgb([220, 40, 40])
            } else {
                Rgb([128, 128, 128])
            }
        });
        assert!(has_meaningful_color(&img));
    }

    #[test]
    fn ycbcr_round_trip_matches_pillows_for_sample_colors() {
        // Image.new('RGB', (1, 1), c).convert('YCbCr') and back, in Pillow.
        let ycbcr = YCbCr::new();
        for (rgb, expected_ycbcr, expected_back) in [
            ([255u8, 0, 0], [76u8, 84, 255], [254u8, 0, 0]),
            ([0, 255, 0], [149, 43, 21], [0, 254, 0]),
            ([0, 0, 255], [29, 255, 107], [0, 0, 254]),
            ([200, 100, 50], [124, 86, 182], [199, 99, 49]),
            ([17, 17, 17], [17, 128, 128], [17, 17, 17]),
        ] {
            assert_eq!(ycbcr.from_rgb(rgb), expected_ycbcr, "{rgb:?}");
            assert_eq!(ycbcr.to_rgb(expected_ycbcr), expected_back, "{rgb:?}");
        }
    }

    #[test]
    fn an_even_tint_is_color_only_when_color_output_was_asked_for() {
        let img = RgbImage::from_pixel(64, 64, Rgb([220, 40, 40]));
        assert!(!has_meaningful_color(&img));
        assert!(has_color_worth_keeping(&img));
        // A truly neutral page is gray either way.
        let gray = RgbImage::from_pixel(64, 64, Rgb([90, 90, 90]));
        assert!(!has_color_worth_keeping(&gray));
    }

    #[test]
    fn a_uniformly_tinted_page_is_not_color() {
        // One flat color, however saturated, has no chroma *spread* at all,
        // and upstream reads a narrow spread as a cast rather than as color.
        let img = RgbImage::from_pixel(64, 64, Rgb([220, 40, 40]));
        assert!(!has_meaningful_color(&img));
    }

    #[test]
    fn a_faint_even_cast_is_not_color() {
        // A slightly warm scan: every pixel off-neutral by the same small
        // amount, so both chroma channels span almost nothing.
        let img = RgbImage::from_fn(64, 64, |x, _| {
            let v = (100 + x) as u8;
            Rgb([v + 3, v, v - 3])
        });
        assert!(!has_meaningful_color(&img));
    }

    fn gray_page_with_one_pixel(pixel: [u8; 3]) -> RgbImage {
        let mut img = RgbImage::from_fn(100, 100, |x, y| {
            let v = ((x + y) % 200) as u8;
            Rgb([v, v, v])
        });
        img.put_pixel(50, 50, Rgb(pixel));
        img
    }

    #[test]
    fn a_single_vivid_pixel_settles_the_untrimmed_pass() {
        // Upstream's first pass trims nothing, so one pixel far enough from
        // neutral (here Cr 255) calls the whole page color. Reproduced as
        // upstream decides it, not as it arguably should.
        assert!(has_meaningful_color(&gray_page_with_one_pixel([255, 0, 0])));
    }

    #[test]
    fn a_mildly_off_neutral_stray_pixel_is_trimmed_away() {
        // Cb 124 / Cr 138: not far enough for the first pass, and gone once
        // the second trims 0.2% of the page (20 pixels) off each end.
        assert!(!has_meaningful_color(&gray_page_with_one_pixel([
            120, 100, 100
        ])));
    }

    #[test]
    fn trimming_takes_the_same_count_from_each_end() {
        let mut histogram = [0u64; 256];
        histogram[0] = 5;
        histogram[1] = 5;
        histogram[100] = 20;
        histogram[254] = 2;
        histogram[255] = 1;
        trim_both_ends(&mut histogram, 7);
        // Low end: all of bin 0, then 2 of bin 1. High end: bins 255 and 254
        // whole, then the remaining 4 out of bin 100.
        assert_eq!(
            (
                histogram[0],
                histogram[1],
                histogram[100],
                histogram[254],
                histogram[255]
            ),
            (0, 3, 16, 0, 0)
        );
    }
}
