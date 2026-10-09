//! JPEG output, the way Pillow writes it when KCC saves a page or a cover (`optimize=1`): the
//! Huffman tables are built from the picture, not the standard ones, which makes the file
//! smaller without changing a pixel (see [`recode`]); and a color picture keeps its chroma at
//! half the width and height (4:2:0), which is libjpeg's default and halves the data the chroma
//! planes take. The quantization tables are the standard ones scaled by the quality, as libjpeg
//! scales them. The pixels are transformed and quantized by the `jpeg-encoder` crate.

mod recode;

use crate::{Error, Result};
use jpeg_encoder::{ColorType, Encoder, ImageBuffer, JpegColorType, SamplingFactor};

/// The samples of a picture handed to [`encode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Samples {
    /// One byte per pixel.
    Gray,
    /// Three bytes per pixel: red, green, blue.
    Rgb,
}

/// Encode a picture as a JPEG at `quality` (1 to 100).
pub(crate) fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    samples: Samples,
    quality: u8,
) -> Result<Vec<u8>> {
    let too_large = || {
        Error::Encode(format!(
            "a JPEG cannot be {width}x{height}: its sides are at most 65535 pixels"
        ))
    };
    let (width_u16, height_u16) = (
        u16::try_from(width).map_err(|_| too_large())?,
        u16::try_from(height).map_err(|_| too_large())?,
    );

    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, quality);
    let encoded = match samples {
        Samples::Gray => encoder.encode(pixels, width_u16, height_u16, ColorType::Luma),
        Samples::Rgb => {
            encoder.set_sampling_factor(SamplingFactor::F_2_2);
            encoder.encode_image(Planes::new(pixels, width as usize, height as usize))
        }
    };
    encoded.map_err(|error| Error::Encode(format!("JPEG: {error}")))?;
    match samples {
        Samples::Gray => drop_chroma_table(&mut bytes),
        Samples::Rgb => number_components_from_one(&mut bytes),
    }
    // The encoder can build the tables itself, but then it writes each component as a scan of
    // its own; libjpeg writes one scan with all of them, and so does this.
    Ok(recode::optimize(&bytes).unwrap_or(bytes))
}

/// The encoder numbers the components of a color picture 0, 1, 2. JFIF, and libjpeg with it,
/// numbers them 1, 2, 3, and a decoder that tells a color space from the numbers (the one the
/// `image` crate uses does, for files without an Adobe marker) takes 0, 1, 2 for something other
/// than Y, Cb, Cr and shows the picture in the wrong colors. The numbers are in the frame header
/// and again in the scan header; nothing else refers to them.
fn number_components_from_one(bytes: &mut [u8]) {
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let end = at + 2 + length;
        if end > bytes.len() {
            return;
        }
        match marker {
            // Frame: precision, height, width, count, then (id, sampling, table) for each.
            0xC0 if length >= 17 && bytes[at + 9] == 3 => {
                for (n, component) in (0..3).zip(1u8..) {
                    bytes[at + 10 + 3 * n] = component;
                }
            }
            // Scan: count, then (id, tables) for each; the data that follows is not a segment.
            0xDA => {
                if length >= 12 && bytes[at + 4] == 3 {
                    for (n, component) in (0..3).zip(1u8..) {
                        bytes[at + 5 + 2 * n] = component;
                    }
                }
                return;
            }
            _ => {}
        }
        at = end;
    }
}

/// A color picture as the planes libjpeg would hand to its encoder: converted to Y, Cb and Cr,
/// and with the chroma planes already reduced to half size in both directions the way libjpeg
/// reduces them. The encoder reduces the chroma it is given by its own average; given chroma that
/// is the same over every 2x2 square (each reduced sample stood for all four), its average is that
/// sample, so the reduction here is the one that counts.
struct Planes {
    width: u16,
    height: u16,
    luma: Vec<u8>,
    blue: Vec<u8>,
    red: Vec<u8>,
}

impl Planes {
    fn new(rgb: &[u8], width: usize, height: usize) -> Self {
        // JFIF's conversion, in the 16-bit fixed point libjpeg uses.
        let convert = |pixel: &[u8; 3]| {
            let (r, g, b) = (
                i32::from(pixel[0]),
                i32::from(pixel[1]),
                i32::from(pixel[2]),
            );
            let y = (19595 * r + 38470 * g + 7471 * b + 0x8000) >> 16;
            let cb = (-11059 * r - 21709 * g + 32768 * b + (128 << 16) + 0x7FFF) >> 16;
            let cr = (32768 * r - 27439 * g - 5329 * b + (128 << 16) + 0x7FFF) >> 16;
            (y as u8, cb as u8, cr as u8)
        };
        let count = width * height;
        let (mut luma, mut blue, mut red) = (
            Vec::with_capacity(count),
            Vec::with_capacity(count),
            Vec::with_capacity(count),
        );
        for pixel in rgb.as_chunks::<3>().0 {
            let (y, cb, cr) = convert(pixel);
            luma.push(y);
            blue.push(cb);
            red.push(cr);
        }
        Self {
            width: width as u16,
            height: height as u16,
            luma,
            blue: reduce_and_repeat(&blue, width, height),
            red: reduce_and_repeat(&red, width, height),
        }
    }
}

/// libjpeg's reduction of a plane to half size both ways: each sample is the average of a 2x2
/// square, rounded with a bias that alternates 1, 2, 1, 2 along the row so that it does not lean
/// one way, and an edge that falls in the middle of a square repeats its last pixel. Returned at
/// the original size, each reduced sample repeated over its square.
fn reduce_and_repeat(plane: &[u8], width: usize, height: usize) -> Vec<u8> {
    let at = |x: usize, y: usize| u32::from(plane[y.min(height - 1) * width + x.min(width - 1)]);
    let mut out = vec![0u8; width * height];
    for y in (0..height).step_by(2) {
        let mut bias = 1;
        for x in (0..width).step_by(2) {
            let sum = at(x, y) + at(x + 1, y) + at(x, y + 1) + at(x + 1, y + 1);
            let sample = ((sum + bias) >> 2) as u8;
            bias ^= 3;
            for dy in 0..2.min(height - y) {
                for dx in 0..2.min(width - x) {
                    out[(y + dy) * width + x + dx] = sample;
                }
            }
        }
    }
    out
}

impl ImageBuffer for Planes {
    fn get_jpeg_color_type(&self) -> JpegColorType {
        JpegColorType::Ycbcr
    }

    fn width(&self) -> u16 {
        self.width
    }

    fn height(&self) -> u16 {
        self.height
    }

    fn fill_buffers(&self, y: u16, buffers: &mut [Vec<u8>; 4]) {
        let row = usize::from(y) * usize::from(self.width)
            ..(usize::from(y) + 1) * usize::from(self.width);
        buffers[0].extend_from_slice(&self.luma[row.clone()]);
        buffers[1].extend_from_slice(&self.blue[row.clone()]);
        buffers[2].extend_from_slice(&self.red[row]);
    }
}

/// The encoder writes both quantization tables whatever the picture. libjpeg writes the ones the
/// picture uses, and a gray picture uses only the first: take the other out, as a program that
/// compares the tables of two files (the comparison with KCC does) would otherwise find them
/// different.
fn drop_chroma_table(bytes: &mut Vec<u8>) {
    let mut at = 2; // after the start-of-image marker
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        if marker == 0xDA {
            return; // the scan: nothing after it is a table
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let end = at + 2 + length;
        if end > bytes.len() {
            return;
        }
        // A table segment of precision 8 bits that holds table 1 and only that.
        if marker == 0xDB && length == 67 && bytes[at + 4] == 0x01 {
            bytes.drain(at..end);
            return;
        }
        at = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32) -> image::RgbImage {
        image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([
                (x * 3 + y) as u8,
                (y * 2 + x / 2) as u8,
                ((x ^ y) * 5) as u8,
            ])
        })
    }

    #[test]
    fn a_gray_picture_round_trips_close_to_itself() {
        let gray = image::DynamicImage::ImageRgb8(picture(64, 48)).to_luma8();
        let bytes = encode(gray.as_raw(), 64, 48, Samples::Gray, 90).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
        assert_eq!(decoded.dimensions(), (64, 48));
        let total: u32 = gray
            .pixels()
            .zip(decoded.pixels())
            .map(|(a, b)| u32::from(a.0[0].abs_diff(b.0[0])))
            .sum();
        assert!(
            total / (64 * 48) < 4,
            "mean difference {}",
            total / (64 * 48)
        );
    }

    #[test]
    fn a_color_picture_keeps_its_chroma_at_half_size_in_both_directions() {
        let source = picture(64, 48);
        let bytes = encode(source.as_raw(), 64, 48, Samples::Rgb, 85).unwrap();
        // The frame header lists each component's sampling factors; luma is 2x2, chroma 1x1.
        let sof = bytes
            .windows(2)
            .position(|marker| marker == [0xFF, 0xC0])
            .expect("a baseline frame");
        let components = &bytes[sof + 10..sof + 19];
        assert_eq!([components[0], components[3], components[6]], [1, 2, 3]);
        let sos = bytes
            .windows(2)
            .position(|marker| marker == [0xFF, 0xDA])
            .unwrap();
        assert_eq!([bytes[sos + 5], bytes[sos + 7], bytes[sos + 9]], [1, 2, 3]);
        assert_eq!(components[1], 0x22, "luma sampling");
        assert_eq!(components[4], 0x11, "chroma sampling");
        assert_eq!(components[7], 0x11, "chroma sampling");
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (64, 48));
    }

    /// The ids of the quantization tables a file defines.
    fn table_ids(bytes: &[u8]) -> Vec<u8> {
        let mut ids = Vec::new();
        let mut at = 2;
        while bytes[at] == 0xFF && bytes[at + 1] != 0xDA {
            let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
            if bytes[at + 1] == 0xDB {
                ids.push(bytes[at + 4] & 0x0F);
            }
            at += 2 + length;
        }
        ids
    }

    #[test]
    fn a_gray_picture_has_one_component_and_one_table() {
        let bytes = encode(&[128; 16 * 16], 16, 16, Samples::Gray, 85).unwrap();
        let sof = bytes
            .windows(2)
            .position(|marker| marker == [0xFF, 0xC0])
            .unwrap();
        assert_eq!(bytes[sof + 9], 1, "number of components");
        assert_eq!(table_ids(&bytes), [0]);
        assert!(image::load_from_memory(&bytes).is_ok());
    }

    #[test]
    fn a_color_picture_keeps_both_tables() {
        let source = picture(32, 32);
        let bytes = encode(source.as_raw(), 32, 32, Samples::Rgb, 85).unwrap();
        assert_eq!(table_ids(&bytes), [0, 1]);
    }

    #[test]
    fn the_huffman_tables_come_from_the_picture() {
        // A flat picture needs few symbols: with tables built from it the file is smaller
        // than with the standard ones, which carry every symbol.
        let flat = vec![200u8; 256 * 256];
        let optimized = encode(&flat, 256, 256, Samples::Gray, 85).unwrap();
        let mut standard = Vec::new();
        let encoder = Encoder::new(&mut standard, 85);
        encoder.encode(&flat, 256, 256, ColorType::Luma).unwrap();
        assert!(optimized.len() < standard.len());
    }

    #[test]
    fn a_side_longer_than_a_jpeg_can_be_is_an_error() {
        assert!(matches!(
            encode(&[0; 70_000], 70_000, 1, Samples::Gray, 85),
            Err(Error::Encode(_))
        ));
    }

    #[test]
    fn the_quality_scales_the_standard_tables() {
        let source = picture(64, 48);
        let low = encode(source.as_raw(), 64, 48, Samples::Rgb, 20).unwrap();
        let high = encode(source.as_raw(), 64, 48, Samples::Rgb, 95).unwrap();
        assert!(low.len() < high.len());
    }

    #[test]
    fn colors_come_back_the_right_ones_through_the_image_crates_decoder() {
        let source = image::RgbImage::from_fn(48, 48, |x, y| match (x / 16, y / 16) {
            (0, _) => image::Rgb([220, 30, 30]),
            (1, _) => image::Rgb([30, 200, 40]),
            _ => image::Rgb([40, 40, 220]),
        });
        let bytes = encode(source.as_raw(), 48, 48, Samples::Rgb, 90).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
        for (x, wanted) in [
            (8, [220i32, 30, 30]),
            (24, [30, 200, 40]),
            (40, [40, 40, 220]),
        ] {
            let got = decoded.get_pixel(x, 24).0;
            for channel in 0..3 {
                assert!(
                    (i32::from(got[channel]) - wanted[channel]).abs() < 12,
                    "{got:?} against {wanted:?}"
                );
            }
        }
    }

    #[test]
    fn the_conversion_to_y_cb_cr_is_jfifs_in_libjpegs_fixed_point() {
        // White, black, red, green.
        let planes = Planes::new(&[255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0], 4, 1);
        assert_eq!(planes.luma, [255, 0, 76, 150]);
        // Cb is 128, 128, 85, 44 and Cr 128, 128, 255, 21; reduced in pairs (a picture one row
        // high repeats its row): (2*128 + 2*128 + 1) / 4, and (2*85 + 2*44 + 2) / 4 for Cb.
        assert_eq!(planes.blue, [128, 128, 65, 65]);
        assert_eq!(planes.red, [128, 128, 138, 138]);
    }

    #[test]
    fn chroma_is_reduced_by_four_with_a_bias_that_alternates() {
        // Two squares of 0 and of 1 in a row: the sums are 0 and 4.
        let plane = [0, 0, 1, 1, 0, 0, 1, 1];
        assert_eq!(reduce_and_repeat(&plane, 4, 2), plane);
        // A square of sum 7 rounds down with bias 1 and up with bias 2: (7+1)>>2 and (7+2)>>2.
        let plane = [2, 2, 2, 1, 2, 2, 2, 1];
        assert_eq!(
            reduce_and_repeat(&plane, 4, 2),
            [2, 2, 1 + 1, 1 + 1, 2, 2, 2, 2]
        );
    }

    #[test]
    fn an_edge_in_the_middle_of_a_square_repeats_its_last_pixel() {
        let plane = [4, 8, 12];
        // First square: 4+8+4+8 = 24, bias 1 -> 6; second: 12 four times = 48, bias 2 -> 12.
        assert_eq!(reduce_and_repeat(&plane, 3, 1), [6, 6, 12]);
    }
}
