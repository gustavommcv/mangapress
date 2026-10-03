//! Pillow's image resampling, reproduced to the bit.
//!
//! Every resize upstream KCC does is a Pillow call — `ImageOps.contain`,
//! `fit`, `pad`, `Image.resize`, `thumbnail` — so "the same page as
//! upstream" means the same resampler, not merely the same filter by name.
//! The `image` crate's Catmull-Rom and Lanczos3 are the same *filters* as
//! Pillow's bicubic and Lanczos, but it evaluates them in floating point and
//! Pillow in 22-bit fixed point with its own rounding, so the two agree to
//! within a level per pixel and no closer. That was the floor under every
//! comparison with upstream until this module replaced it.
//!
//! It also made one case properly wrong rather than noisy: crop-to-fill
//! (`ImageOps.fit`) resamples from a *fractional* source box, which the
//! `image` crate cannot express. The box used to be rounded to whole pixels
//! first, shifting the result by a fraction of a pixel — invisible on a
//! photo, several gray levels on hatching or screentone.
//!
//! What Pillow does (`libImaging/Resample.c`), and this does:
//! - For each output column, the filter is centered on the matching point of
//!   the source — `box_start + (x + 0.5) * scale` — and stretched by the
//!   scale when shrinking, so that it always covers the source pixels that
//!   fall under one output pixel. Its weights over the source pixels in
//!   reach are normalized to sum to one.
//! - The weights are then fixed to 22 fractional bits, rounding half away
//!   from zero, and a pixel is the weighted sum plus one half, shifted back
//!   and clamped to 0..=255.
//! - Columns first, into an 8-bit intermediate (only the rows the second
//!   pass will read), then rows. The intermediate's rounding is part of the
//!   result.
//!
//! Pillow's `reducing_gap` pre-shrink is not here: nothing upstream does
//! with a page uses it (only `thumbnail()`, for the cover; see
//! [`crate::ebook::cover`]).

use image::{ImageBuffer, Pixel};

/// The two filters upstream uses: bicubic when a page is being enlarged (or
/// already fits), Lanczos when it is being shrunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Bicubic,
    Lanczos,
}

impl Filter {
    /// How far from its center the filter reaches, in source pixels at
    /// scale one.
    fn support(self) -> f64 {
        match self {
            Filter::Bicubic => 2.0,
            Filter::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        match self {
            Filter::Bicubic => {
                // The Keys cubic with a = -0.5 (Catmull-Rom).
                const A: f64 = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
            Filter::Lanczos => {
                let sinc = |x: f64| {
                    if x == 0.0 {
                        1.0
                    } else {
                        let x = x * std::f64::consts::PI;
                        x.sin() / x
                    }
                };
                if (-3.0..3.0).contains(&x) {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
        }
    }
}

const PRECISION_BITS: u32 = 32 - 8 - 2;

/// For one axis: for each output position, the first source position it
/// reads, how many it reads, and their fixed-point weights.
struct Axis {
    reach: usize,
    bounds: Vec<(usize, usize)>,
    weights: Vec<i32>,
}

impl Axis {
    fn new(source_size: u32, start: f64, end: f64, output_size: u32, filter: Filter) -> Self {
        let scale = (end - start) / output_size as f64;
        let filter_scale = scale.max(1.0);
        let support = filter.support() * filter_scale;
        let reach = support.ceil() as usize * 2 + 1;

        let mut bounds = Vec::with_capacity(output_size as usize);
        let mut weights = vec![0i32; output_size as usize * reach];
        let mut exact = vec![0f64; reach];
        for position in 0..output_size as usize {
            let center = start + (position as f64 + 0.5) * scale;
            // Truncating casts, as the C is: a negative start rounds toward
            // zero before it is clamped.
            let first = ((center - support + 0.5) as i64).max(0);
            let last = ((center + support + 0.5) as i64).min(source_size as i64);
            let count = (last - first).max(0) as usize;

            let mut total = 0.0;
            for (offset, slot) in exact.iter_mut().enumerate().take(count) {
                let weight =
                    filter.weight((offset as f64 + first as f64 - center + 0.5) / filter_scale);
                *slot = weight;
                total += weight;
            }
            let row = &mut weights[position * reach..(position + 1) * reach];
            for (slot, &weight) in row.iter_mut().zip(&exact).take(count) {
                let weight = if total != 0.0 { weight / total } else { weight };
                let fixed = weight * (1u32 << PRECISION_BITS) as f64;
                *slot = if weight < 0.0 {
                    (fixed - 0.5) as i32
                } else {
                    (fixed + 0.5) as i32
                };
            }
            bounds.push((first as usize, count));
        }
        Axis {
            reach,
            bounds,
            weights,
        }
    }
}

fn clip(sum: i32) -> u8 {
    (sum >> PRECISION_BITS).clamp(0, 255) as u8
}

/// `Image.resize(size, filter, box=source_box)`: `image` — or, with a
/// `source_box` of `[left, top, right, bottom]` in fractional source pixels,
/// just that part of it — resampled to `size`.
pub fn resize<P>(
    image: &ImageBuffer<P, Vec<u8>>,
    size: (u32, u32),
    filter: Filter,
    source_box: Option<[f64; 4]>,
) -> ImageBuffer<P, Vec<u8>>
where
    P: Pixel<Subpixel = u8> + 'static,
{
    let (source_width, source_height) = image.dimensions();
    let (width, height) = size;
    let [left, top, right, bottom] = source_box
        .map(|[left, top, right, bottom]| {
            [
                left.max(0.0),
                top.max(0.0),
                right.min(source_width as f64),
                bottom.min(source_height as f64),
            ]
        })
        .unwrap_or([0.0, 0.0, source_width as f64, source_height as f64]);
    let channels = P::CHANNEL_COUNT as usize;

    let across = width != source_width || left != 0.0 || right != source_width as f64;
    let down = height != source_height || top != 0.0 || bottom != source_height as f64;
    if !across && !down {
        return image.clone();
    }

    let columns = Axis::new(source_width, left, right, width, filter);
    let mut rows = Axis::new(source_height, top, bottom, height, filter);
    // The only source rows the second pass reads.
    let first_row = rows.bounds.first().map_or(0, |&(first, _)| first);
    let last_row = rows
        .bounds
        .last()
        .map_or(source_height as usize, |&(first, count)| first + count);

    let source = image.as_raw();
    let mut current = (source_width as usize, source_height as usize);
    let mut data: Option<Vec<u8>> = None;

    if across {
        let kept_rows = last_row - first_row;
        let mut pass = vec![0u8; width as usize * kept_rows * channels];
        for y in 0..kept_rows {
            let source_row = &source[(y + first_row) * current.0 * channels..];
            for x in 0..width as usize {
                let (first, count) = columns.bounds[x];
                let weights = &columns.weights[x * columns.reach..];
                for channel in 0..channels {
                    let mut sum = 1i32 << (PRECISION_BITS - 1);
                    for offset in 0..count {
                        sum += source_row[(first + offset) * channels + channel] as i32
                            * weights[offset];
                    }
                    pass[(y * width as usize + x) * channels + channel] = clip(sum);
                }
            }
        }
        for bound in rows.bounds.iter_mut() {
            bound.0 -= first_row;
        }
        current = (width as usize, kept_rows);
        data = Some(pass);
    }

    if down {
        let input = data.as_deref().unwrap_or(source);
        let mut pass = vec![0u8; current.0 * height as usize * channels];
        for y in 0..height as usize {
            let (first, count) = rows.bounds[y];
            let weights = &rows.weights[y * rows.reach..];
            for x in 0..current.0 {
                for channel in 0..channels {
                    let mut sum = 1i32 << (PRECISION_BITS - 1);
                    for offset in 0..count {
                        sum += input[((first + offset) * current.0 + x) * channels + channel]
                            as i32
                            * weights[offset];
                    }
                    pass[(y * current.0 + x) * channels + channel] = clip(sum);
                }
            }
        }
        current = (current.0, height as usize);
        data = Some(pass);
    }

    ImageBuffer::from_raw(
        current.0 as u32,
        current.1 as u32,
        data.expect("at least one pass ran"),
    )
    .expect("each pass fills its buffer exactly")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    /// A small image from a formula, so the same input can be rebuilt in
    /// Python to ask Pillow for its answer.
    fn source(width: u32, height: u32) -> GrayImage {
        GrayImage::from_fn(width, height, |x, y| {
            Luma([((x * 37 + y * 91 + (x * y) % 7 * 23) % 256) as u8])
        })
    }

    // Each expectation below is real Pillow's output for `source`:
    // `Image.fromarray(...).resize(size, filter, box=...)`.

    #[test]
    fn shrinking_with_lanczos_is_pillows_pixel_for_pixel() {
        let out = resize(&source(9, 7), (4, 3), Filter::Lanczos, None);
        assert_eq!(
            out.as_raw(),
            &[82, 105, 147, 90, 133, 132, 121, 114, 147, 168, 144, 141]
        );
    }

    #[test]
    fn shrinking_with_bicubic_is_pillows_pixel_for_pixel() {
        let out = resize(&source(9, 7), (4, 3), Filter::Bicubic, None);
        assert_eq!(
            out.as_raw(),
            &[85, 104, 149, 93, 127, 133, 125, 109, 148, 164, 146, 141]
        );
    }

    #[test]
    fn enlarging_with_bicubic_is_pillows_pixel_for_pixel() {
        let out = resize(&source(5, 4), (8, 6), Filter::Bicubic, None);
        assert_eq!(
            out.as_raw(),
            &[
                0, 6, 31, 52, 81, 115, 141, 156, 35, 60, 102, 141, 123, 59, 85, 116, 107, 118, 142,
                199, 159, 34, 47, 77, 193, 115, 19, 69, 122, 154, 123, 93, 102, 80, 63, 128, 176,
                186, 110, 50, 0, 50, 138, 218, 234, 180, 77, 8
            ]
        );
    }

    #[test]
    fn a_fractional_source_box_is_pillows_pixel_for_pixel() {
        // box=(1.25, 0.5, 7.75, 6.5): what crop-to-fill resamples from.
        let out = resize(
            &source(9, 7),
            (4, 3),
            Filter::Lanczos,
            Some([1.25, 0.5, 7.75, 6.5]),
        );
        assert_eq!(
            out.as_raw(),
            &[106, 99, 146, 125, 166, 111, 130, 98, 160, 143, 168, 136]
        );
    }

    #[test]
    fn resizing_one_axis_only_skips_the_other_pass() {
        let out = resize(&source(9, 7), (9, 3), Filter::Lanczos, None);
        assert_eq!(
            out.as_raw(),
            &[
                73, 69, 132, 88, 111, 159, 162, 76, 72, 107, 123, 182, 131, 46, 207, 84, 110, 126,
                113, 177, 165, 135, 205, 130, 135, 116, 180
            ]
        );
    }

    #[test]
    fn the_same_size_and_no_box_is_a_copy() {
        let image = source(6, 5);
        assert_eq!(resize(&image, (6, 5), Filter::Lanczos, None), image);
    }

    #[test]
    fn every_channel_of_a_color_image_is_resampled_alike() {
        let gray = source(9, 7);
        let rgb = image::RgbImage::from_fn(9, 7, |x, y| {
            let v = gray.get_pixel(x, y)[0];
            image::Rgb([v, 255 - v, v / 2])
        });
        let out = resize(&rgb, (4, 3), Filter::Lanczos, None);
        let expected = resize(&gray, (4, 3), Filter::Lanczos, None);
        for (pixel, gray) in out.pixels().zip(expected.pixels()) {
            assert_eq!(pixel[0], gray[0]);
        }
    }
}
