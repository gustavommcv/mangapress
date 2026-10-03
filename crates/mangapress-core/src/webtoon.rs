//! Webtoon mode: a chapter of long vertical strips, cut into screen-sized
//! pages between panels instead of across them.
//!
//! Port target: `mergeDirectory()` and `splitImage()` in KCC's
//! `comic2panel.py`, which upstream runs over the whole book (`-m -i`)
//! before any page processing when `--webtoon` is on. Reimplemented from its
//! behavior rather than translated (the same caution as
//! `docs/adr/0007-gplv3-boundary-kcc-image-rs.md` asks for), and checked
//! against the real thing: both stages only ever copy pixels, so their
//! output here is expected to equal upstream's exactly, not approximately.
//!
//! Two steps per chapter folder:
//! 1. [`merge_strip`] stacks every image of the folder into one tall strip.
//! 2. [`split_strip`] finds the panels in it — runs of rows with something
//!    drawn in them, between runs of flat color — and packs whole panels
//!    into pages no taller than the screen, splitting only a panel that is
//!    too tall for one.

use crate::error::{Error, Result};
use image::imageops::FilterType;
use image::{GrayImage, RgbImage};

/// Pillow refuses to build an image taller than this; upstream reports it
/// as the chapter being too long.
const MAX_STRIP_HEIGHT: u64 = 131_072 * 4;

/// Upstream measures page height against a screen at most this wide.
const MAX_VIRTUAL_WIDTH: u32 = 1072;

/// Stacks a folder's images, top to bottom, into one strip as wide as most
/// of them are. An image of any other width is resized to the strip's
/// width first (bicubic), keeping its proportions.
///
/// When two widths are equally common upstream takes whichever a Python
/// set happens to yield first, which is not a rule; the narrower one is
/// taken here.
pub fn merge_strip(pages: Vec<RgbImage>) -> Result<RgbImage> {
    let Some(strip_width) = most_common_width(&pages) else {
        return Err(Error::Webtoon("no images to merge".to_string()));
    };
    let total_height: u64 = pages.iter().map(|page| page.height() as u64).sum();
    if total_height > MAX_STRIP_HEIGHT {
        return Err(Error::Webtoon(format!(
            "strip too tall at {total_height} pixels ({strip_width} wide); try separate chapter folders"
        )));
    }

    // The canvas is sized from the images' *original* heights, as upstream
    // sizes it, even though a resized image may come out shorter or taller:
    // what doesn't fit is cut off, what is left over stays black.
    let mut strip = RgbImage::new(strip_width, total_height as u32);
    let mut y = 0i64;
    for page in pages {
        let page = if page.width() == strip_width {
            page
        } else {
            let height = (page.height() as f64 * (strip_width as f64 / page.width() as f64)) as u32;
            crate::resize::fit(&page, (strip_width, height.max(1)), FilterType::CatmullRom)
        };
        image::imageops::overlay(&mut strip, &page, 0, y);
        y += page.height() as i64;
    }
    Ok(strip)
}

/// The whole step for one chapter folder: its images, still encoded, in;
/// the pages they become, as PNG, out. PNG because upstream writes them so
/// (they are processed like any other source page afterwards), and RGB even
/// from grayscale sources, as upstream's strip is.
pub fn pages_from_chapter(sources: &[&[u8]], device: (u32, u32)) -> Result<Vec<Vec<u8>>> {
    use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
    use image::ImageEncoder;

    let mut decoded = Vec::with_capacity(sources.len());
    for source in sources {
        decoded.push(image::load_from_memory(source)?.to_rgb8());
    }
    let strip = merge_strip(decoded)?;

    let mut pages = Vec::new();
    for page in split_strip(&strip, device)? {
        let mut bytes = Vec::new();
        // An intermediate file that is decoded again straight away: fast to
        // write matters more than small.
        PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, PngFilter::NoFilter)
            .write_image(
                page.as_raw(),
                page.width(),
                page.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        pages.push(bytes);
    }
    Ok(pages)
}

fn most_common_width(pages: &[RgbImage]) -> Option<u32> {
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    for page in pages {
        *counts.entry(page.width()).or_default() += 1;
    }
    let most = counts.values().copied().max()?;
    counts
        .into_iter()
        .find(|&(_, count)| count == most)
        .map(|(width, _)| width)
}

/// Cuts a strip into pages for a `device`-sized screen. A strip no taller
/// than the screen is returned whole.
///
/// The cut points come from an edge map of the strip: a window a few rows
/// tall slides down it, and wherever the window holds nothing but flat
/// color the strip is between panels. Each panel is then one page, or part
/// of one — as many whole panels as fit are stacked on a page, in order —
/// and a panel too tall for a page is covered by several page-high slices
/// that overlap rather than leave a remainder.
///
/// Quirks kept from upstream, all of which show in its output: a strip with
/// nothing but flat color produces no pages at all; a page 15 pixels tall
/// or less is dropped; and a short run of drawn rows at the very top of the
/// strip is not counted as a panel.
pub fn split_strip(strip: &RgbImage, device: (u32, u32)) -> Result<Vec<RgbImage>> {
    let (width, height) = strip.dimensions();
    if width < 300 {
        return Err(Error::Webtoon(format!(
            "a {width}px-wide strip is too narrow to split (300px at least)"
        )));
    }
    if height <= device.1 {
        return Ok(vec![strip.clone()]);
    }

    let drawn = drawn_pixels(&crate::color::to_gray(strip));
    let panels = find_panels(&drawn);
    let page_height = virtual_page_height(width, device);
    let slices = slice_tall_panels(&panels, page_height);

    // Whole slices are stacked on a page for as long as they fit.
    let mut pages: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut room = page_height;
    for (index, slice) in slices.iter().enumerate() {
        if room - slice.height > 0 {
            room -= slice.height;
            current.push(index);
        } else {
            if !current.is_empty() {
                pages.push(std::mem::take(&mut current));
            }
            room = page_height - slice.height;
            current.push(index);
        }
    }
    if !current.is_empty() {
        pages.push(current);
    }

    let mut output = Vec::with_capacity(pages.len());
    for page in pages {
        let total: i64 = page.iter().map(|&index| slices[index].height).sum();
        if total <= 15 {
            continue;
        }
        let mut canvas = RgbImage::new(width, total as u32);
        let mut y = 0i64;
        for &index in &page {
            let slice = slices[index];
            let top = slice.top.clamp(0, height as i64) as u32;
            let bottom = slice.bottom.clamp(0, height as i64) as u32;
            if bottom > top {
                let rows = image::imageops::crop_imm(strip, 0, top, width, bottom - top).to_image();
                // A slice that starts above the strip is black there, as a
                // Pillow crop outside its image is.
                image::imageops::overlay(&mut canvas, &rows, 0, y + (top as i64 - slice.top));
            }
            y += slice.height;
        }
        output.push(canvas);
    }
    Ok(output)
}

/// Where something is drawn: Pillow's `FIND_EDGES` filter (each pixel times
/// eight, minus its eight neighbours, clamped to 0..=255) over the strip's
/// grayscale, thresholded above 6. As in Pillow, the outermost row and
/// column on each side are not filtered but copied — so along the border
/// this is simply "the pixel is not near-black", which is why a light strip
/// always looks drawn-on in its first and last row.
fn drawn_pixels(gray: &GrayImage) -> GrayImage {
    let (w, h) = gray.dimensions();
    let source = gray.as_raw();
    let at = |x: u32, y: u32| source[(y * w + x) as usize] as i32;
    GrayImage::from_fn(w, h, |x, y| {
        let value = if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
            at(x, y)
        } else {
            let neighbours = at(x - 1, y - 1)
                + at(x, y - 1)
                + at(x + 1, y - 1)
                + at(x - 1, y)
                + at(x + 1, y)
                + at(x - 1, y + 1)
                + at(x, y + 1)
                + at(x + 1, y + 1);
            (8 * at(x, y) - neighbours).clamp(0, 255)
        };
        image::Luma([if value > 6 { 255 } else { 0 }])
    })
}

/// A panel: the rows `top..bottom` of the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Panel {
    top: i64,
    bottom: i64,
}

/// A page-high (or shorter) piece of the strip to copy onto a page. Its
/// `height` is what it occupies there, which upstream tracks separately
/// from `bottom - top`; the two agree for everything [`slice_tall_panels`]
/// produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slice {
    top: i64,
    bottom: i64,
    height: i64,
}

/// Slides a window down the edge map and reports the runs of rows where it
/// is not flat. The window is one eightieth of the strip's width tall
/// (rounded up to even), leaves a twentieth of the width out on each side,
/// and moves by half its own height.
fn find_panels(drawn: &GrayImage) -> Vec<Panel> {
    let (width, height) = (drawn.width() as i64, drawn.height() as i64);
    let side_margin = width / 20;
    let mut window = width / 80;
    if window % 2 == 1 {
        window += 1;
    }
    let step = (window / 2).max(1);

    // Flat means all one value. Rows past the bottom of the strip count as
    // black, as they do in a Pillow crop that runs off the image.
    let flat = |top: i64| {
        let (mut any_black, mut any_white) = (top + window > height, false);
        for y in top..(top + window).min(height) {
            for x in side_margin..(width - side_margin) {
                if drawn.get_pixel(x as u32, y as u32)[0] == 0 {
                    any_black = true;
                } else {
                    any_white = true;
                }
                if any_black && any_white {
                    return false;
                }
            }
        }
        true
    };

    let mut panels = Vec::new();
    let mut open: Option<i64> = None;
    let mut y = 0i64;
    while y < height {
        let is_flat = flat(y);
        if !is_flat && open.is_none() {
            open = Some(y);
        }
        if height - y <= step && !is_flat {
            if let Some(top) = open.take() {
                panels.push(Panel {
                    top,
                    bottom: height,
                });
            }
        }
        if is_flat {
            if let Some(top) = open.take() {
                // A short run at the very top of the strip is the border
                // artifact described on `drawn_pixels`, not a panel.
                if !(top < window * 2 && y - top < window * 2) {
                    panels.push(Panel { top, bottom: y });
                }
            }
        }
        y += step;
    }
    panels
}

/// The page height pages are packed to: the device's proportions, at the
/// narrowest of the device's width, the strip's width and 1072 pixels.
fn virtual_page_height(strip_width: u32, device: (u32, u32)) -> i64 {
    let virtual_width = MAX_VIRTUAL_WIDTH.min(device.0).min(strip_width) as f64;
    let reference_width = if device.0 > MAX_VIRTUAL_WIDTH {
        MAX_VIRTUAL_WIDTH
    } else {
        device.0
    } as f64;
    (device.1 as f64 / reference_width * virtual_width) as i64
}

/// Panels as pieces no taller than a page. A panel up to one and a half
/// pages tall is left whole (it is shrunk to fit later); up to two pages,
/// it becomes a top slice and a bottom slice, each a page tall, overlapping
/// in the middle; taller still, evenly spaced page-tall slices from its top
/// to its bottom.
fn slice_tall_panels(panels: &[Panel], page_height: i64) -> Vec<Slice> {
    let mut slices = Vec::new();
    for panel in panels {
        let height = panel.bottom - panel.top;
        if height as f64 <= page_height as f64 * 1.5 {
            slices.push(Slice {
                top: panel.top,
                bottom: panel.bottom,
                height,
            });
        } else if height <= page_height * 2 {
            slices.push(Slice {
                top: panel.top,
                bottom: panel.bottom - (height - page_height),
                height: page_height,
            });
            slices.push(Slice {
                top: panel.bottom - page_height,
                bottom: panel.bottom,
                height: page_height,
            });
        } else {
            let parts = (height as f64 / page_height as f64).ceil() as i64;
            let spacing = height / parts;
            slices.push(Slice {
                top: panel.top,
                bottom: panel.top + page_height,
                height: page_height,
            });
            for part in 1..parts - 1 {
                let top = panel.top + part * spacing;
                slices.push(Slice {
                    top,
                    bottom: top + page_height,
                    height: page_height,
                });
            }
            slices.push(Slice {
                top: panel.bottom - page_height,
                bottom: panel.bottom,
                height: page_height,
            });
        }
    }
    slices
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    const DEVICE: (u32, u32) = (1072, 1448);

    /// A white strip `width` wide with a textured block for each
    /// `(top, height)` in `panels`.
    fn strip_with_panels(width: u32, height: u32, panels: &[(u32, u32)]) -> RgbImage {
        RgbImage::from_fn(width, height, |x, y| {
            let inside = panels
                .iter()
                .any(|&(top, panel_height)| y >= top && y < top + panel_height);
            if inside {
                let v = ((x * 7 + y * 13) % 200) as u8;
                Rgb([v, v, v])
            } else {
                Rgb([255, 255, 255])
            }
        })
    }

    #[test]
    fn merging_stacks_images_at_the_most_common_width() {
        let pages = vec![
            RgbImage::from_pixel(800, 100, Rgb([10, 10, 10])),
            RgbImage::from_pixel(800, 200, Rgb([20, 20, 20])),
            RgbImage::from_pixel(400, 100, Rgb([30, 30, 30])),
        ];
        let strip = merge_strip(pages).unwrap();
        // 100 + 200 + 100 rows of canvas; the 400x100 image becomes 800x200
        // and its lower half falls off the end.
        assert_eq!(strip.dimensions(), (800, 400));
        assert_eq!(strip.get_pixel(0, 50)[0], 10);
        assert_eq!(strip.get_pixel(0, 250)[0], 20);
        assert_eq!(strip.get_pixel(0, 350)[0], 30);
    }

    #[test]
    fn a_strip_that_fits_the_screen_is_left_whole() {
        let strip = strip_with_panels(800, 1000, &[(100, 300)]);
        assert_eq!(split_strip(&strip, DEVICE).unwrap(), vec![strip]);
    }

    #[test]
    fn a_narrow_strip_is_refused() {
        let strip = RgbImage::new(200, 5000);
        assert!(matches!(
            split_strip(&strip, DEVICE),
            Err(Error::Webtoon(_))
        ));
    }

    #[test]
    fn page_height_follows_the_devices_proportions_at_the_strips_width() {
        assert_eq!(virtual_page_height(800, DEVICE), 1080);
        assert_eq!(virtual_page_height(2000, DEVICE), 1448);
        // A device wider than 1072 is measured as if it were 1072 wide.
        assert_eq!(virtual_page_height(2000, (1264, 1680)), 1680);
        assert_eq!(virtual_page_height(800, (1264, 1680)), 1253);
    }

    #[test]
    fn tall_panels_are_sliced_into_overlapping_page_high_pieces() {
        let slices = |height: i64| {
            slice_tall_panels(
                &[Panel {
                    top: 100,
                    bottom: 100 + height,
                }],
                1000,
            )
        };
        // Up to one and a half pages: left whole.
        assert_eq!(slices(1500).len(), 1);
        // Up to two: a top page and a bottom page, overlapping.
        assert_eq!(
            slices(1800),
            [
                Slice {
                    top: 100,
                    bottom: 1100,
                    height: 1000
                },
                Slice {
                    top: 900,
                    bottom: 1900,
                    height: 1000
                },
            ]
        );
        // Beyond: evenly spaced from top to bottom.
        assert_eq!(
            slices(3500),
            [
                Slice {
                    top: 100,
                    bottom: 1100,
                    height: 1000
                },
                Slice {
                    top: 975,
                    bottom: 1975,
                    height: 1000
                },
                Slice {
                    top: 1850,
                    bottom: 2850,
                    height: 1000
                },
                Slice {
                    top: 2600,
                    bottom: 3600,
                    height: 1000
                },
            ]
        );
    }

    #[test]
    fn panels_are_cut_apart_at_the_flat_gaps_and_packed_onto_pages() {
        // Page height for an 800px strip is 1080. Three panels of 500, 400
        // and 700 rows with white gaps: the first two share a page, the
        // third starts the next.
        let strip = strip_with_panels(800, 2400, &[(200, 500), (900, 400), (1500, 700)]);
        let pages = split_strip(&strip, DEVICE).unwrap();
        assert!(pages.len() >= 2, "{} pages", pages.len());
        assert!(pages.iter().all(|page| page.width() == 800));
        assert!(pages.iter().all(|page| page.height() as i64 <= 1080));
        // Nothing drawn is lost: every panel row is on some page.
        let drawn_rows: u32 = pages
            .iter()
            .map(|page| {
                (0..page.height())
                    .filter(|&y| page.get_pixel(400, y)[0] != 255)
                    .count() as u32
            })
            .sum();
        assert!(
            drawn_rows >= 500 + 400 + 700 - 30,
            "{drawn_rows} drawn rows kept"
        );
    }

    #[test]
    fn a_strip_of_flat_color_yields_no_pages() {
        // Upstream's own behavior: no panel found, nothing written.
        let strip = RgbImage::from_pixel(800, 4000, Rgb([0, 0, 0]));
        assert!(split_strip(&strip, DEVICE).unwrap().is_empty());
    }
}
