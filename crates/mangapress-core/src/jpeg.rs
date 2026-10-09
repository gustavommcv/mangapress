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
#[path = "jpeg/tests.rs"]
mod tests;
