//! Resource checks at the boundary where file bytes and image headers are trusted.
//! See ADR 0015 for the policy, its reference, and what these limits do not bound.

use crate::{Error, Result};
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use std::io::{Cursor, Read};
use std::path::Path;

/// Maximum uncompressed bytes in one input file or archive entry (256 MiB).
pub const MAX_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

/// Twice KCC 12.0.0's page parser's `Image.MAX_IMAGE_PIXELS` override.
/// This is deliberately larger than Pillow's default; see ADR 0015.
pub const MAX_IMAGE_PIXELS: u64 = 2 * 715_827_882;

/// KCC 12.0.0's webtoon splitter turns the warning above this area into an error.
pub const MAX_WEBTOON_PIXELS: u64 = 1_000_000_000;

/// Read a source file without allocating from its reported size. Used for
/// pages, metadata, separately supplied covers, and spread-label files.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let declared_size = file.metadata()?.len();
    read_entry(file, path, declared_size)
}

pub(crate) fn read_entry(reader: impl Read, path: &Path, declared_size: u64) -> Result<Vec<u8>> {
    read_bounded(reader, path, declared_size, MAX_ENTRY_BYTES)
}

fn read_bounded(reader: impl Read, path: &Path, declared_size: u64, limit: u64) -> Result<Vec<u8>> {
    let too_large = || Error::InputTooLarge {
        path: path.to_path_buf(),
        limit,
    };
    if declared_size > limit {
        return Err(too_large());
    }
    // The header is only an early rejection, never an allocation hint or
    // the final authority: stop after one byte beyond the actual limit.
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

fn check_dimensions(width: u32, height: u32, limit: u64) -> Result<()> {
    if u64::from(width) * u64::from(height) > limit {
        return Err(Error::ImageTooLarge {
            width,
            height,
            limit,
        });
    }
    Ok(())
}

fn decoder(bytes: &[u8], pixel_limit: u64) -> Result<impl ImageDecoder + '_> {
    let decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height, pixel_limit)?;
    Ok(decoder)
}

/// Check image headers without decoding pixels (also used by passthrough mode).
pub(crate) fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    Ok(decoder(bytes, MAX_IMAGE_PIXELS)?.dimensions())
}

/// A page as far as its file went.
pub(crate) struct DecodedPage {
    pub image: DynamicImage,
    /// The file ended, or its data broke, before the whole image was read. What was not read
    /// is blank, as Pillow leaves it when KCC asks it to load truncated images (`image.py`).
    pub truncated: bool,
}

/// Decode through the image crate, preserving its default allocation checks
/// as well as checking the pixel count before allocating the output buffer.
fn decode_whole(bytes: &[u8]) -> Result<DynamicImage> {
    let mut decoder = decoder(bytes, MAX_IMAGE_PIXELS)?;
    let mut limits = Limits::default();
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    Ok(DynamicImage::from_decoder(decoder)?)
}

/// Decode a page. A PNG or GIF whose pixel data ends early is not refused, as KCC does not
/// refuse it: the part that was read is kept and the rest is blank (black, or the first
/// color of a palette, as Pillow's buffer starts out), and the result says so.
pub(crate) fn decode_page(bytes: &[u8]) -> Result<DecodedPage> {
    match decode_whole(bytes) {
        Ok(image) => Ok(DecodedPage {
            image,
            truncated: false,
        }),
        Err(Error::Image(error)) if data_ended_early(&error) => match decode_partly(bytes) {
            Some(image) => Ok(DecodedPage {
                image,
                truncated: true,
            }),
            None => Err(Error::Image(error)),
        },
        Err(error) => Err(error),
    }
}

/// Decode a page, whole or as far as it goes (see [`decode_page`]).
pub(crate) fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    Ok(decode_page(bytes)?.image)
}

/// A decoder met the end of the file, or data it could not read, after the header was read.
/// Limits, unsupported formats and wrong parameters are not that.
fn data_ended_early(error: &image::ImageError) -> bool {
    match error {
        image::ImageError::IoError(error) => error.kind() == std::io::ErrorKind::UnexpectedEof,
        image::ImageError::Decoding(_) => true,
        _ => false,
    }
}

/// What a decoder had written when it stopped, handed back as a decoder so that the image
/// crate turns it into an image the way it does for any other.
struct Partial {
    width: u32,
    height: u32,
    color: image::ColorType,
    pixels: Vec<u8>,
}

impl ImageDecoder for Partial {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn color_type(&self) -> image::ColorType {
        self.color
    }

    fn read_image(self, buf: &mut [u8]) -> image::ImageResult<()> {
        buf.copy_from_slice(&self.pixels);
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> image::ImageResult<()> {
        (*self).read_image(buf)
    }
}

/// Decode again, keeping what the decoder wrote before it stopped. Only for the formats in
/// which Pillow keeps what it read when a file ends early (PNG and GIF; JPEG decoders keep it
/// by themselves). `None` for the others, when it stops for a reason that is not the end of
/// the data, or when it does not stop.
fn decode_partly(bytes: &[u8]) -> Option<DynamicImage> {
    let mut decoder = decoder(bytes, MAX_IMAGE_PIXELS).ok()?;
    let mut limits = Limits::default();
    limits.reserve(decoder.total_bytes()).ok()?;
    decoder.set_limits(limits).ok()?;
    let (width, height) = decoder.dimensions();
    let color = decoder.color_type();
    // Pillow reads a WebP whole or not at all, and KCC does not ask for BMP pages.
    let format = image::guess_format(bytes)
        .ok()
        .filter(|format| matches!(format, image::ImageFormat::Png | image::ImageFormat::Gif))?;
    let png = format == image::ImageFormat::Png;

    let mut pixels = vec![0u8; usize::try_from(decoder.total_bytes()).ok()?];
    let unread = match format {
        image::ImageFormat::Png => png_unread_pixel(bytes, color),
        image::ImageFormat::Gif => gif_unread_pixel(bytes),
        _ => None,
    };
    if let Some(unread) =
        unread.filter(|unread| unread.len() == usize::from(color.bytes_per_pixel()))
    {
        for pixel in pixels.chunks_exact_mut(unread.len()) {
            pixel.copy_from_slice(&unread);
        }
    }
    match decoder.read_image(&mut pixels) {
        Err(error) if data_ended_early(&error) => {}
        _ => return None,
    }
    // The PNG decoder puts 16-bit samples in the machine's order only when it succeeds.
    if png && color.bytes_per_pixel() / color.channel_count() == 2 {
        pixels
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .for_each(|sample| *sample = u16::from_be_bytes(*sample).to_ne_bytes());
    }
    DynamicImage::from_decoder(Partial {
        width,
        height,
        color,
        pixels,
    })
    .ok()
}

/// The pixel a GIF has before its data is read, as the decoder writes it (RGBA): what Pillow's
/// buffer holds before it is filled. That is the transparent index when the first image has one
/// (a transparent pixel), and otherwise index 0 of its palette.
fn gif_unread_pixel(bytes: &[u8]) -> Option<Vec<u8>> {
    let table = |at: usize, packed: u8| -> Option<(&[u8], usize)> {
        if packed & 0x80 == 0 {
            return None;
        }
        let length = 3 * (2usize << (packed & 7));
        Some((bytes.get(at..at + length)?, at + length))
    };
    let screen_packed = *bytes.get(10)?;
    let (global, mut at) = match table(13, screen_packed) {
        Some((colors, next)) => (Some(colors), next),
        None => (None, 13),
    };
    let mut transparent = None;
    loop {
        match *bytes.get(at)? {
            0x21 => {
                if *bytes.get(at + 1)? == 0xF9 && *bytes.get(at + 2)? >= 4 {
                    let flags = *bytes.get(at + 3)?;
                    transparent = (flags & 1 == 1)
                        .then(|| bytes.get(at + 6).copied())
                        .flatten();
                }
                at += 2;
                loop {
                    let size = usize::from(*bytes.get(at)?);
                    at += size + 1;
                    if size == 0 {
                        break;
                    }
                }
            }
            0x2C => {
                let local = table(at + 10, *bytes.get(at + 9)?);
                let colors = local.map(|(colors, _)| colors).or(global)?;
                // Pillow starts the first frame as the transparent index when there is one.
                let first = 3 * usize::from(transparent.unwrap_or(0));
                let alpha = if transparent.is_some() { 0 } else { 255 };
                return Some(vec![
                    *colors.get(first)?,
                    *colors.get(first + 1)?,
                    *colors.get(first + 2)?,
                    alpha,
                ]);
            }
            _ => return None,
        }
    }
}

/// The pixel a PNG has before its data is read, in the layout the decoder writes, when it is not
/// all zeros: Pillow's buffer starts as index 0 of the palette, and a gray or RGB image with a
/// transparent color starts opaque unless that color is black. Samples are big-endian, as the
/// decoder leaves them.
fn png_unread_pixel(bytes: &[u8], color: image::ColorType) -> Option<Vec<u8>> {
    let reader = png::Decoder::new(Cursor::new(bytes)).read_info().ok()?;
    let info = reader.info();
    let bytes_per_sample = color.bytes_per_pixel() / color.channel_count();
    let mut pixel = vec![0u8; usize::from(color.bytes_per_pixel())];
    let alpha_at = pixel.len() - usize::from(bytes_per_sample);
    match info.color_type {
        png::ColorType::Indexed => {
            let palette = info.palette.as_deref()?;
            pixel[..3].copy_from_slice(palette.get(..3)?);
            if color.has_alpha() {
                pixel[alpha_at..].fill(
                    info.trns
                        .as_deref()
                        .and_then(|t| t.first())
                        .copied()
                        .unwrap_or(255),
                );
            }
        }
        png::ColorType::Grayscale | png::ColorType::Rgb if color.has_alpha() => {
            let transparent = info
                .trns
                .as_deref()
                .is_some_and(|t| t.iter().all(|&b| b == 0));
            pixel[alpha_at..].fill(if transparent { 0 } else { 255 });
        }
        _ => return None,
    }
    Some(pixel)
}

pub(crate) fn check_webtoon_dimensions(width: u32, height: u32) -> Result<()> {
    check_dimensions(width, height, MAX_WEBTOON_PIXELS)
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
