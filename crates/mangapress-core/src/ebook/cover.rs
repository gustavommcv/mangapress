//! The book's cover image, built from its untouched first image.
//!
//! Upstream reference: `Cover` in KCC's `image.py` (GPLv3 upstream — reimplemented
//! from documented behavior, not copied; see
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! The cover is not a page. Upstream makes it from the source file of the
//! book's first image, before and apart from page processing, and so does
//! this: whatever happens to the first *page* — its margins cropped, a wide
//! one split in two or turned on its side — the cover stays the whole
//! image, upright. An earlier version declared the processed first page as
//! the cover instead, to save writing a second image; that saved about
//! 120 KB a book and, for a first image wide enough to be split, made the
//! cover its first half — in right-to-left order the right half, which on a
//! wraparound jacket is the back.
//!
//! What upstream does to it, in order: autocontrast (always, color or not —
//! unlike a page), grayscale, optionally cut the front cover out of a wide
//! image (`--smartcovercrop`), then either shrink it to fit the device
//! (never enlarging) or crop it to fill the device exactly (`--coverfill`),
//! and save it as JPEG at the pages' quality.
//!
//! One known difference: when the image is at least four times the device's
//! size, Pillow's `thumbnail()` first averages it down by a whole factor and
//! only then resamples. That pre-step is not reproduced; the image is
//! resampled in one pass.

use crate::error::Result;
use crate::resize::round_half_even;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder};

type Buffer<P> = image::ImageBuffer<P, Vec<u8>>;

#[derive(Debug, Clone, Copy)]
pub struct CoverOptions {
    /// The device resolution the cover is fitted to.
    pub target: (u32, u32),
    /// Which half of a wide image holds the front cover, for `smart_crop`.
    pub right_to_left: bool,
    /// `--smartcovercrop`: cut the front cover out of a wide image.
    pub smart_crop: bool,
    /// `--coverfill`: crop to fill the device instead of fitting inside it.
    pub fill: bool,
    /// `--forcecolor`: keep the cover in color.
    pub force_color: bool,
    pub jpeg_quality: u8,
}

/// The cover for a book whose first source image is `source_bytes`, encoded
/// as JPEG — grayscale, or in the image's own colors with `force_color`.
pub fn build_cover(source_bytes: &[u8], options: &CoverOptions) -> Result<Vec<u8>> {
    build_cover_reporting(source_bytes, options).map(|(bytes, _)| bytes)
}

/// [`build_cover`], also saying whether `smart_crop` actually cut a front
/// cover out of the image (it leaves one that is not wider than tall
/// alone). Upstream puts a smart-cropped cover into a CBZ as well, as its
/// first image.
pub fn build_cover_reporting(
    source_bytes: &[u8],
    options: &CoverOptions,
) -> Result<(Vec<u8>, bool)> {
    let source = crate::input::decode_image(source_bytes)?.to_rgb8();
    let stretched = crate::contrast::autocontrast_preserving_tone(&source);
    if options.force_color {
        finish_cover(stretched, ExtendedColorType::Rgb8, options)
    } else {
        finish_cover(
            crate::color::to_gray(&stretched),
            ExtendedColorType::L8,
            options,
        )
    }
}

fn finish_cover<P: image::Pixel<Subpixel = u8> + 'static>(
    cover: Buffer<P>,
    color_type: ExtendedColorType,
    options: &CoverOptions,
) -> Result<(Vec<u8>, bool)> {
    let smart_cropped = options.smart_crop && cover.width() > cover.height();
    let cover = if options.smart_crop {
        cut_front_cover(cover, options.right_to_left)
    } else {
        cover
    };
    let cover = if options.fill {
        crate::resize::fit(&cover, options.target, FilterType::Lanczos3)
    } else {
        match thumbnail_size(cover.dimensions(), options.target) {
            Some((width, height)) => {
                crate::resize::resample(&cover, (width, height), FilterType::Lanczos3, None)
            }
            None => cover,
        }
    };

    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut std::io::Cursor::new(&mut bytes), options.jpeg_quality)
        .write_image(cover.as_raw(), cover.width(), cover.height(), color_type)?;
    Ok((bytes, smart_cropped))
}

/// Upstream's `crop_main_cover()`: the front cover's place in a wide first
/// image, by how wide the image is. Each band of aspect ratios is a kind of
/// scan — from a full jacket with flaps (over 2:1) down to a plain two-page
/// spread — with the fraction of the width upstream found the front cover
/// to occupy in it; right-to-left books have it on the left of the spine,
/// left-to-right ones on the right. An image no wider than it is tall is
/// already a cover and is returned whole.
fn cut_front_cover<P: image::Pixel<Subpixel = u8> + 'static>(
    image: Buffer<P>,
    right_to_left: bool,
) -> Buffer<P> {
    let (w, h) = (image.width() as f64, image.height() as f64);
    let ratio = w / h;
    let (left, right) = if ratio > 2.0 {
        if right_to_left {
            (w / 6.0, w / 2.0 - w * 0.02)
        } else {
            (w / 2.0 + w * 0.02, 5.0 / 6.0 * w)
        }
    } else if ratio > 1.83 {
        if right_to_left {
            (w * 0.19, w * 0.575)
        } else {
            (w * 0.425, 0.81 * w)
        }
    } else if ratio > 1.7 {
        if right_to_left {
            (w * 0.2, w * 0.583)
        } else {
            (w * 0.417, 0.8 * w)
        }
    } else if ratio > 1.34 {
        if right_to_left {
            (0.0, w / 2.0 - w * 0.03)
        } else {
            (w / 2.0 + w * 0.03, w)
        }
    } else if ratio > 1.0 {
        if right_to_left {
            (w * 0.36, w)
        } else {
            (0.0, 0.64 * w)
        }
    } else {
        return image;
    };

    // Pillow's crop rounds each edge half to even.
    let (left, right) = (round_half_even(left), round_half_even(right));
    image::imageops::crop_imm(&image, left, 0, right.saturating_sub(left).max(1), h as u32)
        .to_image()
}

/// The size `Image.thumbnail(target)` shrinks an image to, or `None` when
/// the image already fits and is left alone — a thumbnail never enlarges.
///
/// Pillow keeps the target's limiting dimension and gives the other the
/// whole number, of the two around its exact value, that keeps the aspect
/// ratio closer — not simply the rounded value, and the lower of the two on
/// a tie. Never less than one pixel.
fn thumbnail_size((w, h): (u32, u32), (target_w, target_h): (u32, u32)) -> Option<(u32, u32)> {
    if target_w >= w && target_h >= h {
        return None;
    }
    let aspect = w as f64 / h as f64;
    let (target_w_f, target_h_f) = (target_w as f64, target_h as f64);
    let closer_to_aspect = |exact: f64, aspect_with: &dyn Fn(f64) -> f64| -> u32 {
        let (below, above) = (exact.floor(), exact.ceil());
        let error = |candidate: f64| {
            if candidate == 0.0 {
                0.0
            } else {
                (aspect - aspect_with(candidate)).abs()
            }
        };
        let chosen = if error(above) < error(below) {
            above
        } else {
            below
        };
        (chosen as u32).max(1)
    };

    Some(if target_w_f / target_h_f >= aspect {
        let width = closer_to_aspect(target_h_f * aspect, &|width| width / target_h_f);
        (width, target_h)
    } else {
        let height = closer_to_aspect(target_w_f / aspect, &|height| target_w_f / height);
        (target_w, height)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma, Rgb, RgbImage};

    fn png(image: RgbImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn options() -> CoverOptions {
        CoverOptions {
            target: (1072, 1448),
            right_to_left: true,
            smart_crop: false,
            fill: false,
            force_color: false,
            jpeg_quality: 100,
        }
    }

    fn decode(bytes: &[u8]) -> GrayImage {
        image::load_from_memory(bytes).unwrap().to_luma8()
    }

    #[test]
    fn thumbnail_sizes_match_pillows() {
        // `Image.new('L', size).thumbnail((1072, 1448))` in real Pillow.
        for (size, expected) in [
            ((900, 1350), (900, 1350)),
            ((1072, 1448), (1072, 1448)),
            ((2000, 3000), (965, 1448)),
            ((1500, 1448), (1072, 1035)),
            ((3000, 2000), (1072, 715)),
            ((1073, 1448), (1072, 1447)),
            ((2144, 2897), (1072, 1448)),
            ((1000, 3001), (483, 1448)),
            ((4000, 37), (1072, 10)),
            ((1601, 2401), (966, 1448)),
        ] {
            let size_after = thumbnail_size(size, (1072, 1448)).unwrap_or(size);
            assert_eq!(size_after, expected, "{size:?}");
        }
    }

    #[test]
    fn smart_cover_crop_matches_upstreams_boxes() {
        // The left edge and width real KCC 12.0.0 crops to, for one image in
        // each of its aspect bands: (size, right-to-left, left-to-right).
        for (size, rtl, ltr) in [
            ((3000u32, 1400u32), (500u32, 940u32), (1560u32, 940u32)),
            ((2600, 1400), (494, 1001), (1105, 1001)),
            ((2450, 1400), (490, 938), (1022, 938)),
            ((2000, 1400), (0, 940), (1060, 940)),
            ((1500, 1400), (540, 960), (0, 960)),
            ((1400, 1400), (0, 1400), (0, 1400)),
            ((900, 1350), (0, 900), (0, 900)),
        ] {
            for (right_to_left, (left, width)) in [(true, rtl), (false, ltr)] {
                // Every pixel holds its own column, in 5-column steps, so the
                // crop's left edge can be read back out of the result.
                let image = GrayImage::from_fn(size.0, size.1, |x, _| Luma([(x / 5 % 256) as u8]));
                let cropped = cut_front_cover(image, right_to_left);
                assert_eq!(
                    cropped.dimensions(),
                    (width, size.1),
                    "{size:?} rtl={right_to_left}"
                );
                assert_eq!(
                    cropped.get_pixel(0, 0)[0],
                    (left / 5 % 256) as u8,
                    "{size:?} rtl={right_to_left}"
                );
            }
        }
    }

    #[test]
    fn a_cover_is_stretched_to_full_contrast_even_when_it_is_color() {
        // Values 60..180 with a saturated patch: a *page* like this is left
        // alone for being color; the cover is autocontrasted regardless.
        let source = RgbImage::from_fn(300, 450, |x, y| {
            if x < 100 && y < 100 {
                Rgb([180, 60, 60])
            } else if y % 30 < 15 {
                Rgb([60, 60, 60])
            } else {
                Rgb([180, 180, 180])
            }
        });
        let cover = decode(&build_cover(&png(source), &options()).unwrap());
        let (low, high) = cover
            .pixels()
            .fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        assert!(low < 5 && high > 250, "{low}..{high}");
    }

    #[test]
    fn an_oversized_cover_is_refused_before_pixel_processing() {
        assert!(matches!(
            build_cover(&crate::test_support::oversized_bmp(), &options()),
            Err(crate::Error::ImageTooLarge { .. })
        ));
    }

    #[test]
    fn a_cover_is_shrunk_to_fit_the_device_but_never_enlarged() {
        let small = RgbImage::from_pixel(300, 450, Rgb([128, 128, 128]));
        let cover = decode(&build_cover(&png(small), &options()).unwrap());
        assert_eq!(cover.dimensions(), (300, 450));

        let large = RgbImage::from_pixel(2000, 3000, Rgb([128, 128, 128]));
        let cover = decode(&build_cover(&png(large), &options()).unwrap());
        assert_eq!(cover.dimensions(), (965, 1448));
    }

    #[test]
    fn a_wide_first_image_stays_whole_unless_smart_crop_is_asked_for() {
        let jacket = RgbImage::from_pixel(3000, 1400, Rgb([128, 128, 128]));
        let whole = decode(&build_cover(&png(jacket.clone()), &options()).unwrap());
        assert_eq!(whole.dimensions(), (1072, 500));

        let mut smart = options();
        smart.smart_crop = true;
        // 940x1400 fits the device already, so the crop is the cover.
        let front = decode(&build_cover(&png(jacket), &smart).unwrap());
        assert_eq!(front.dimensions(), (940, 1400));
    }

    #[test]
    fn cover_fill_crops_to_exactly_the_device() {
        let mut fill = options();
        fill.fill = true;
        let source = RgbImage::from_pixel(2000, 2000, Rgb([128, 128, 128]));
        let cover = decode(&build_cover(&png(source), &fill).unwrap());
        assert_eq!(cover.dimensions(), (1072, 1448));
    }

    #[test]
    fn force_color_keeps_the_cover_in_color() {
        let source = RgbImage::from_fn(300, 450, |x, _| {
            if x < 150 {
                Rgb([200, 40, 40])
            } else {
                Rgb([40, 40, 200])
            }
        });
        let gray = image::load_from_memory(&build_cover(&png(source.clone()), &options()).unwrap())
            .unwrap();
        assert!(!gray.color().has_color());

        let mut in_color = options();
        in_color.force_color = true;
        let cover =
            image::load_from_memory(&build_cover(&png(source), &in_color).unwrap()).unwrap();
        assert!(cover.color().has_color());
        let left = cover.to_rgb8().get_pixel(40, 200).0;
        assert!(left[0] > left[2], "red on the left: {left:?}");
    }
}
