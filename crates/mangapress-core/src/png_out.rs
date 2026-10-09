//! Gray and color PNG pages, written the way Pillow writes them when KCC saves a page
//! (`optimize=1`): the strongest compression, and for each row the filter that suits it best.
//! The default of the encoder in use is a fast, light compression that makes files several
//! times larger for the same pixels.

use crate::Result;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, ImageEncoder};

/// Encode 8-bit gray (`L8`) or color (`Rgb8`) samples as a PNG.
pub(crate) fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    color: ExtendedColorType,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Best, FilterType::Adaptive)
        .write_image(pixels, width, height, color)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pixels_survive_and_the_file_is_smaller_than_the_encoders_default() {
        let gray =
            image::GrayImage::from_fn(200, 200, |x, y| image::Luma([((x / 8 + y / 8) * 9) as u8]));
        let bytes = encode(gray.as_raw(), 200, 200, ExtendedColorType::L8).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
        assert_eq!(decoded, gray);

        let mut plain = Vec::new();
        image::DynamicImage::ImageLuma8(gray)
            .write_to(
                &mut std::io::Cursor::new(&mut plain),
                image::ImageFormat::Png,
            )
            .unwrap();
        assert!(
            bytes.len() < plain.len(),
            "{} against {}",
            bytes.len(),
            plain.len()
        );
    }

    #[test]
    fn color_pixels_survive() {
        let rgb =
            image::RgbImage::from_fn(64, 40, |x, y| image::Rgb([x as u8, y as u8, (x ^ y) as u8]));
        let bytes = encode(rgb.as_raw(), 64, 40, ExtendedColorType::Rgb8).unwrap();
        assert_eq!(image::load_from_memory(&bytes).unwrap().to_rgb8(), rgb);
    }
}
