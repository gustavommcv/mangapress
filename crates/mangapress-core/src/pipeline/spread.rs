//! Double-page spread detection: split vs. rotate vs. leave alone.
//!
//! Port target: `ComicPageParser.splitCheck()` in KCC's `image.py` (GPLv3
//! upstream — spec only, see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`).
//!
//! Decision tree, given source `(width, height)` and device `(dst_width,
//! dst_height)`:
//! 1. Not a spread at all (`Decision::Normal`) unless *both*:
//!    - orientation mismatches the device (`(width > height) != (dst_width >
//!      dst_height)`), and
//!    - `width / height > SPREAD_ASPECT_THRESHOLD` (1.16).
//! 2. If it is a spread: `width / height >= BISECT_THRESHOLD` (1.8) means
//!    "too wide to usefully split" -> rotate instead, even in default Split
//!    mode. Below 1.8, respect the user's `-r/--splitter` choice (Split /
//!    Rotate / Both).
//! 3. Which half becomes "page one" when splitting depends on reading
//!    direction — see [`crate::manga`].
//!
//! Webtoon mode bypasses this entirely (pages pass through as `Normal`).

pub const SPREAD_ASPECT_THRESHOLD: f64 = 1.16;
pub const BISECT_THRESHOLD: f64 = 1.8;

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
    let is_spread = (src_landscape != dst_landscape) && (w / h > SPREAD_ASPECT_THRESHOLD);

    if !is_spread {
        return Decision::Normal;
    }

    let too_wide_to_split = w / h >= BISECT_THRESHOLD;
    use crate::pipeline::SplitterMode::*;
    match (too_wide_to_split, splitter) {
        (true, _) => Decision::Rotate,
        (false, Split) => Decision::Split,
        (false, Rotate) => Decision::Rotate,
        (false, Both) => Decision::Both,
    }
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
}
