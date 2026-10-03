//! Grayscale palette quantization with dithering, and the containers a
//! quantized page is stored in (`--forcepng` output path — the default JPEG
//! path ships full 8-bit tone, undithered).
//!
//! Upstream reference: `quantizeImage()` and the PNG/GIF branch of
//! `save_with_codec()` in KCC's `image.py` (GPLv3 upstream — reimplemented
//! from documented behavior, not copied; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`), plus the three
//! `convertToGrayscale()` exceptions in `imgFileProcessing()`.
//!
//! Upstream's quantization is one Pillow call, `Image.quantize(palette=...)`
//! on the page converted back to RGB, with no `dither=` argument — so the
//! dithering is Pillow's default Floyd-Steinberg, and *which* pixels land on
//! which level is decided by Pillow's implementation of it, not by the
//! textbook algorithm. [`quantize_to_palette_indices`] reproduces that
//! implementation, quirks included, because the two do not agree: an
//! earlier version here diffused the error in floating point with an exact
//! nearest-level search, and against real KCC 12.0.0 it put 16-37% of a real
//! page's pixels on the neighbouring level. This one was checked against
//! real Pillow on synthetic pages and on a full 965x1448 page for all three
//! palettes: not one pixel differs.
//!
//! Storage is where this deliberately stops following upstream. KCC stores
//! a quantized page as a GIF when the device is a Kindle and the book an
//! EPUB — the case where it expects the book to go on to Amazon's converter
//! — and as a palette PNG otherwise. mangapress writes the palette PNG for
//! every device: it holds the same pixels, KOReader reads it on a Kindle
//! like on anything else, and on a real 50-page chapter it came out 7%
//! smaller than the GIF. See [`Container`].

use crate::error::{Error, Result};
use crate::profile::Palette;
use image::{GrayImage, Luma};

/// How a quantized page is stored: as a palette PNG at the smallest bit
/// depth the palette fits in (which is what Pillow writes for upstream), or
/// as plain 8-bit grayscale where upstream converts the quantized page back
/// to grayscale first — PDF output, CBZ output for the four oldest Kindles,
/// `--pnglegacy` — or never quantizes it (`--noquantize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// Palette PNG: 2 bits per pixel for a 4-level palette, 4 for 15 or 16.
    IndexedPng,
    /// 8-bit grayscale PNG.
    GrayPng,
}

/// Quantizes `page` to `palette` the way Pillow does, returning each pixel's
/// index into [`Palette::level_values`], row by row.
///
/// What Pillow does, and this does with it:
/// - It works on RGB, three channels side by side, even though every input
///   pixel is neutral. That matters because of the row-end quirk below,
///   which lets the channels drift apart.
/// - A pixel's working value is its own value plus one sixteenth of the
///   error owed to it — integer division, truncating toward zero — clamped
///   to 0..=255.
/// - The palette entry is not the nearest to that working value but the
///   nearest to it *rounded down to a multiple of 4 on each channel*: Pillow
///   answers from a cache with one slot per 4x4x4 block of RGB values, and
///   fills a slot for the block's lowest corner. Ties go to the lower index.
/// - The error (working value minus the chosen entry) is spread in the
///   usual 7/16 right, 3/16 below-left, 5/16 below, 1/16 below-right — kept
///   as whole sixteenths until it is used, never as a fraction.
/// - At the end of each row, the error carried under the last pixel is
///   written from the wrong variables: all three channels' slots receive
///   the *blue* channel's three running values, rather than each channel
///   receiving its own. It reads like a slip in Pillow's source, and it is
///   why the last column — and, row by row, the pixels left of it — is not
///   what a single-channel dither would produce. Reproduced, since the goal
///   is upstream's output; "fixing" it was tried and differs from Pillow on
///   up to 7% of a small page's pixels.
pub fn quantize_to_palette_indices(page: &GrayImage, palette: Palette) -> Vec<u8> {
    let levels = palette.level_values();
    let (width, height) = (page.width() as usize, page.height() as usize);
    let source = page.as_raw();
    let mut indices = vec![0u8; width * height];

    // Error owed to each pixel of the next row, in sixteenths, per channel.
    // Slot `x + 1` belongs to pixel `x`; the extra slot is the row end's.
    let mut owed_below = vec![[0i32; 3]; width + 1];
    let mut nearest = NearestLevel::new(levels);

    for y in 0..height {
        // Per channel: sixteenths owed to the next pixel, and the two
        // partial sums still being built for the row below.
        let mut owed_right = [0i32; 3];
        let mut below = [0i32; 3];
        let mut below_right = [0i32; 3];
        let mut last_error = [0i32; 3];

        for x in 0..width {
            let value = source[y * width + x] as i32;
            let mut working = [0i32; 3];
            for channel in 0..3 {
                working[channel] =
                    (value + (owed_right[channel] + owed_below[x + 1][channel]) / 16).clamp(0, 255);
            }

            let index = nearest.index_for(working);
            indices[y * width + x] = index;
            let level = levels[index as usize] as i32;

            for channel in 0..3 {
                let error = working[channel] - level;
                last_error[channel] = error;
                owed_below[x][channel] = 3 * error + below[channel];
                below[channel] = 5 * error + below_right[channel];
                below_right[channel] = error;
                owed_right[channel] = 7 * error;
            }
        }

        // The row-end quirk: three values of the blue channel, one per slot.
        owed_below[width] = [below[2], below_right[2], last_error[2]];
    }
    indices
}

/// [`quantize_to_palette_indices`], as an image of the palette's gray values.
/// Every output pixel is guaranteed to be one of `palette.level_values()`.
pub fn quantize_with_floyd_steinberg(page: &GrayImage, palette: Palette) -> GrayImage {
    let levels = palette.level_values();
    let indices = quantize_to_palette_indices(page, palette);
    let mut pixels = indices.iter().map(|&index| levels[index as usize]);
    GrayImage::from_fn(page.width(), page.height(), |_, _| {
        Luma([pixels
            .next()
            .expect("one index per pixel, read in row order")])
    })
}

/// Pillow's palette cache for a gray palette: one answer per 4x4x4 block of
/// RGB values, computed on first use for the block's lowest corner.
struct NearestLevel<'a> {
    levels: &'a [u8],
    cache: Vec<u8>,
}

impl<'a> NearestLevel<'a> {
    const UNSET: u8 = u8::MAX;

    fn new(levels: &'a [u8]) -> Self {
        NearestLevel {
            levels,
            cache: vec![Self::UNSET; 64 * 64 * 64],
        }
    }

    fn index_for(&mut self, [r, g, b]: [i32; 3]) -> u8 {
        let slot = ((r >> 2) + (g >> 2) * 64 + (b >> 2) * 64 * 64) as usize;
        if self.cache[slot] == Self::UNSET {
            let corner = [r & !3, g & !3, b & !3];
            let mut best = (0u8, i32::MAX);
            for (index, &level) in self.levels.iter().enumerate() {
                let distance: i32 = corner
                    .iter()
                    .map(|&c| (c - level as i32) * (c - level as i32))
                    .sum();
                // Strictly closer only: a tie keeps the lower index.
                if distance < best.1 {
                    best = (index as u8, distance);
                }
            }
            self.cache[slot] = best.0;
        }
        self.cache[slot]
    }
}

/// The palette as the RGB triplets a PNG `PLTE` chunk holds.
fn palette_rgb(palette: Palette) -> Vec<u8> {
    palette
        .level_values()
        .iter()
        .flat_map(|&level| [level, level, level])
        .collect()
}

/// A palette PNG at the bit depth Pillow picks for a palette this size: 1
/// bit for up to 2 entries, 2 for up to 4, 4 for up to 16.
pub fn encode_indexed_png(
    (width, height): (u32, u32),
    indices: &[u8],
    palette: Palette,
) -> Result<Vec<u8>> {
    let (depth, bits) = match palette.level_values().len() {
        0..=2 => (png::BitDepth::One, 1usize),
        3..=4 => (png::BitDepth::Two, 2),
        _ => (png::BitDepth::Four, 4),
    };

    // Each row packed most-significant-bits first and padded to a whole byte.
    let row_bytes = (width as usize * bits).div_ceil(8);
    let mut packed = vec![0u8; row_bytes * height as usize];
    for (y, row) in indices.chunks(width as usize).enumerate() {
        for (x, &index) in row.iter().enumerate() {
            let bit = x * bits;
            packed[y * row_bytes + bit / 8] |= index << (8 - bits - bit % 8);
        }
    }

    let encode = |bytes: &mut Vec<u8>| -> std::result::Result<(), png::EncodingError> {
        let mut encoder = png::Encoder::new(bytes, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(depth);
        encoder.set_palette(palette_rgb(palette));
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&packed)?;
        writer.finish()
    };
    let mut bytes = Vec::new();
    encode(&mut bytes).map_err(|error| Error::Encode(error.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page generated from a formula, so the same input can be rebuilt in
    /// Python to ask Pillow for its answer.
    fn synthetic_page(width: u32, height: u32) -> GrayImage {
        GrayImage::from_fn(width, height, |x, y| {
            Luma([((x * 7 + y * 13 + (x * y) % 31 * 5) % 256) as u8])
        })
    }

    fn as_hex_rows(indices: &[u8], width: usize) -> Vec<String> {
        indices
            .chunks(width)
            .map(|row| row.iter().map(|index| format!("{index:x}")).collect())
            .collect()
    }

    // The three expectations below are real Pillow's output for
    // `synthetic_page`: `Image.fromarray(page).convert('RGB')
    // .quantize(palette=...)`, the call upstream makes, one hex digit per
    // pixel's palette index.

    #[test]
    fn sixteen_level_dither_is_pillows_pixel_for_pixel() {
        let page = synthetic_page(40, 24);
        let indices = quantize_to_palette_indices(&page, Palette::Gray16);
        assert_eq!(
            as_hex_rows(&indices, 40),
            [
                "001122233444556677788999aabbcccddeeef001",
                "1123445667899abbcddef0123345567def022344",
                "134457799bbcee0289bbddf01334577f01234567",
                "245689abde089acce0134bce013457801345789a",
                "34689bce78abdf13acef1456e024578124678ac4",
                "4689cd08acd02ace034ce1357e1357914579bd67",
                "479bd09bd02bcf24ce146e1358035792479bd68b",
                "58ad09bd19ce24c035d13681469257a368ad69be",
                "69ce8be1ad04cf25e1571369368b57a479c69be8",
                "7ad1ad1ad14d14d148148158a48b48b48be8be8b",
                "8be3c04d15e25f3714725826947a48b59c09d1be",
                "8c1ae2c04e261482614826a48c6ad8c6ad8c0ad2",
                "9d2c15f4d2715f4826b5948c7a59e8c7bf9e2c1b",
                "ae3d3d2626050484837c7b6a5ae9d8c8c1b0bf4e",
                "b05040404f494959494948d9d8d9d9d8d2d3d2d2",
                "b162727383849495a5a6b7b7c8c8d8e9e4e40505",
                "c3d4e5062738495a6c7c8e9fb0c2d3ea0b1c2d4e",
                "d3f517385a6384a6c8dafb8d9fb1d3eb1c3e5162",
                "e417396284a7c96b8da7c9fb2ea1c3fb2e4173f5",
                "e6285285b85b8ea8db7da1da1c40d40c4063f639",
                "074a75b85b86c96da1db2eb2fc30c41d41852853",
                "1753964b86ca7eb80da2ec31d52f641e53074286",
                "2964b97db97db91ec91ec31ec41f642e742964c9",
                "2a86db97fcb81eca20db32fc531e6420754b975c",
            ]
        );
    }

    #[test]
    fn fifteen_level_dither_is_pillows_pixel_for_pixel() {
        let indices = quantize_to_palette_indices(&synthetic_page(40, 6), Palette::Gray15);
        assert_eq!(
            as_hex_rows(&indices, 40),
            [
                "001122233444556677788999aabbcccdddeee001",
                "1123445667899abbcddee0123345567dee012334",
                "134457799bbcde0289bbcde01334577e01234567",
                "245689abdd189abdd0134cce013457801345789a",
                "34689bce78abde13acde1356d124578124579ac4",
                "4689cd08acd02acd134ce1357e1357823589bd58",
            ]
        );
    }

    #[test]
    fn four_level_dither_is_pillows_pixel_for_pixel() {
        let indices = quantize_to_palette_indices(&synthetic_page(40, 6), Palette::Gray4);
        assert_eq!(
            as_hex_rows(&indices, 40),
            [
                "0000010111111111121222222222232323333000",
                "0011111111122222323330001111111333000111",
                "0101112122323300222223300011112300111111",
                "1111222233012223300112230010121001011222",
                "0112222312223300223301113001112011122221",
                "1122230222300233011230111301112011122212",
            ]
        );
    }

    #[test]
    fn the_nearest_level_is_judged_from_the_value_rounded_down_to_a_multiple_of_four() {
        let levels = Palette::Gray16.level_values();
        let mut nearest = NearestLevel::new(levels);
        // 100 is already a multiple of 4: nearest of 0x55 (85) and 0x66 (102).
        assert_eq!(levels[nearest.index_for([100; 3]) as usize], 0x66);
        // 95 is nearer to 102 than to 85, but Pillow asks about 92, which
        // isn't.
        assert_eq!(levels[nearest.index_for([95; 3]) as usize], 0x55);
    }

    #[test]
    fn gray15s_gap_is_respected_during_quantization() {
        // 0xee is missing from the 15-level palette: 232 sits between 0xdd
        // (221) and 0xff (255), nearer the former.
        let levels = Palette::Gray15.level_values();
        let mut nearest = NearestLevel::new(levels);
        assert_eq!(levels[nearest.index_for([232; 3]) as usize], 0xdd);
        assert_eq!(levels[nearest.index_for([244; 3]) as usize], 0xff);
    }

    #[test]
    fn every_output_pixel_is_an_exact_palette_value() {
        let img = GrayImage::from_fn(32, 32, |x, y| Luma([((x * 8 + y * 3) % 256) as u8]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        let allowed = Palette::Gray16.level_values();
        assert!(quantized.pixels().all(|p| allowed.contains(&p[0])));
    }

    #[test]
    fn flat_image_at_an_exact_palette_value_is_unchanged() {
        let img = GrayImage::from_pixel(16, 16, Luma([0x88]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        assert!(quantized.pixels().all(|p| p[0] == 0x88));
    }

    #[test]
    fn dithered_average_approximates_the_source_value() {
        // Halfway between two levels: the dither has to mix them, and the
        // mix has to average out near the source.
        let img = GrayImage::from_pixel(64, 64, Luma([8]));
        let quantized = quantize_with_floyd_steinberg(&img, Palette::Gray16);
        assert!(quantized.pixels().any(|p| p[0] == 0));
        assert!(quantized.pixels().any(|p| p[0] == 0x11));
        let mean = quantized.pixels().map(|p| p[0] as f64).sum::<f64>() / (64.0 * 64.0);
        assert!((mean - 8.0).abs() < 1.5, "mean {mean}");
    }

    #[test]
    fn indexed_png_decodes_back_to_the_palette_values_at_the_palettes_bit_depth() {
        for (palette, depth) in [
            (Palette::Gray16, png::BitDepth::Four),
            (Palette::Gray15, png::BitDepth::Four),
            (Palette::Gray4, png::BitDepth::Two),
        ] {
            // 37 wide: a row that doesn't end on a byte boundary at either depth.
            let page = synthetic_page(37, 11);
            let indices = quantize_to_palette_indices(&page, palette);
            let bytes = encode_indexed_png((37, 11), &indices, palette).unwrap();

            let reader = png::Decoder::new(std::io::Cursor::new(&bytes))
                .read_info()
                .unwrap();
            assert_eq!(reader.info().color_type, png::ColorType::Indexed);
            assert_eq!(reader.info().bit_depth, depth);

            let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
            assert_eq!(decoded, quantize_with_floyd_steinberg(&page, palette));
        }
    }
}
