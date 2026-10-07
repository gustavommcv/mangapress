//! Joining two source pages that are the halves of one double-page spread.
//!
//! Some scans store a spread as two files, one per page. Left like that,
//! each half is processed as a page of its own: its inner edge is cropped
//! as a margin, and nothing downstream knows the two belong together. Told
//! which pages they are, this puts each pair back into one wide image
//! before anything else happens, so that the pipeline sees a spread and
//! treats it as one — splits it, rotates it, or keeps it whole.
//!
//! Upstream reference: what `makeBook()` in KCC's `comic2ebook.py` does when
//! a `<source>.json` file — written by its "Label Spreads" window — sits
//! beside the source. The file lists the position, counting from zero over
//! the whole book in reading order, of the first page of each pair.
//!
//! Where this follows upstream exactly: which page goes on which side (the
//! first-read page on the right in a right-to-left book, on the left
//! otherwise), both pages against the top edge, a black canvas, and the
//! joined image taking the first page's place.
//!
//! Where it deliberately doesn't:
//! - Upstream flattens the book into one folder first, which drops every
//!   chapter from the table of contents. Here the joined page stays in the
//!   first page's chapter and the chapters stay.
//! - For two pages of different sizes upstream's canvas takes the right-hand
//!   page's height and places that page at its own width from the left, so
//!   a taller left page is cut off and unequal widths overlap or leave a
//!   gap. Here the canvas holds both pages whole, side by side. For pages of
//!   the same size — what a spread's halves are — the two agree.
//! - A position that cannot be used (no page after it, or its page was
//!   already taken by the pair before) stops upstream with an error. Here
//!   it is skipped and reported.

use super::{Chapter, Page};
use crate::error::Result;
use image::{ImageFormat, RgbImage};

/// Why a labelled position was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    /// It is the last page of the book, or past it: there is no page to
    /// join it with.
    NoPageAfter,
    /// Its page is the second half of the pair labelled just before it.
    PartOfPreviousPair,
}

/// What [`join_labelled_spreads`] did with the positions it was given.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Joined {
    /// The positions that were joined with the page after them, ascending.
    pub joined: Vec<usize>,
    pub skipped: Vec<(usize, Skipped)>,
}

impl Joined {
    /// Where a source page ended up: its position among the book's pages
    /// after joining. Both halves of a pair map to the joined page.
    pub fn position_after(&self, position: usize) -> usize {
        position
            - self
                .joined
                .iter()
                .filter(|&&first| first < position)
                .count()
    }
}

/// Joins the page at each of `first_pages` — positions counted from zero
/// over all of `chapters`' pages in order — with the page after it. A
/// chapter left without pages is removed.
pub fn join_labelled_spreads(
    chapters: &mut Vec<Chapter>,
    first_pages: &[usize],
    right_to_left: bool,
) -> Result<Joined> {
    let total: usize = chapters.iter().map(|chapter| chapter.pages.len()).sum();
    let mut positions = first_pages.to_vec();
    positions.sort_unstable();
    positions.dedup();

    let mut outcome = Joined::default();
    for position in positions {
        if position + 1 >= total {
            outcome.skipped.push((position, Skipped::NoPageAfter));
        } else if position > 0 && outcome.joined.last() == Some(&(position - 1)) {
            outcome
                .skipped
                .push((position, Skipped::PartOfPreviousPair));
        } else {
            outcome.joined.push(position);
        }
    }

    // Last pair first, so that the positions before it still mean what they
    // meant when the list was written.
    for &position in outcome.joined.iter().rev() {
        let (chapter, page) = locate(chapters, position);
        let (next_chapter, next_page) = locate(chapters, position + 1);
        let bytes = join(
            &chapters[chapter].pages[page].bytes,
            &chapters[next_chapter].pages[next_page].bytes,
            right_to_left,
        )?;
        chapters[next_chapter].pages.remove(next_page);
        chapters[chapter].pages[page] = Page {
            extension: "png".to_string(),
            bytes,
            ..Default::default()
        };
    }
    chapters.retain(|chapter| !chapter.pages.is_empty());
    Ok(outcome)
}

/// The chapter and the page within it at `position` in the whole book.
fn locate(chapters: &[Chapter], mut position: usize) -> (usize, usize) {
    for (index, chapter) in chapters.iter().enumerate() {
        if position < chapter.pages.len() {
            return (index, position);
        }
        position -= chapter.pages.len();
    }
    unreachable!("positions are checked against the page count before use")
}

/// The two pages side by side as one lossless image.
fn join(first: &[u8], second: &[u8], right_to_left: bool) -> Result<Vec<u8>> {
    let first = crate::input::decode_image(first)?.to_rgb8();
    let second = crate::input::decode_image(second)?.to_rgb8();
    let (left, right) = if right_to_left {
        (second, first)
    } else {
        (first, second)
    };

    let mut canvas = RgbImage::new(
        left.width() + right.width(),
        left.height().max(right.height()),
    );
    image::imageops::replace(&mut canvas, &left, 0, 0);
    image::imageops::replace(&mut canvas, &right, left.width() as i64, 0);

    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(canvas)
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn either_half_of_a_spread_is_checked_before_joining() {
        let oversized = crate::test_support::oversized_bmp();
        let ordinary = page((3, 7), 128);
        for (first, second) in [(&oversized, &ordinary.bytes), (&ordinary.bytes, &oversized)] {
            assert!(matches!(
                join(first, second, false),
                Err(crate::Error::ImageTooLarge { .. })
            ));
        }
    }

    /// A page of one flat color, as PNG.
    fn page(size: (u32, u32), level: u8) -> Page {
        let image = RgbImage::from_pixel(size.0, size.1, image::Rgb([level; 3]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        Page {
            extension: "png".to_string(),
            bytes,
            ..Default::default()
        }
    }

    /// Chapters of 10x20 pages whose gray level is ten times their position
    /// in the book, so a page can be recognized wherever it ends up.
    fn book(pages_per_chapter: &[usize]) -> Vec<Chapter> {
        let mut position = 0;
        pages_per_chapter
            .iter()
            .enumerate()
            .map(|(index, &count)| Chapter {
                relative_path: PathBuf::from(format!("c{index}")),
                title: format!("Chapter {index}"),
                pages: (0..count)
                    .map(|_| {
                        position += 1;
                        page((10, 20), (position - 1) as u8 * 10)
                    })
                    .collect(),
            })
            .collect()
    }

    fn decode(page: &Page) -> RgbImage {
        image::load_from_memory(&page.bytes).unwrap().to_rgb8()
    }

    /// The gray levels found at the left and right ends of a page.
    fn sides(page: &Page) -> (u8, u8) {
        let image = decode(page);
        (
            image.get_pixel(0, 0)[0],
            image.get_pixel(image.width() - 1, 0)[0],
        )
    }

    #[test]
    fn a_right_to_left_pair_has_the_first_read_page_on_the_right() {
        let mut chapters = book(&[4]);
        let outcome = join_labelled_spreads(&mut chapters, &[1], true).unwrap();
        assert_eq!(outcome.joined, [1]);
        assert_eq!(chapters[0].pages.len(), 3);
        assert_eq!(decode(&chapters[0].pages[1]).dimensions(), (20, 20));
        // Page 1 (level 10) on the right, page 2 (level 20) on the left.
        assert_eq!(sides(&chapters[0].pages[1]), (20, 10));
        // The pages around it are untouched and in order.
        assert_eq!(sides(&chapters[0].pages[0]), (0, 0));
        assert_eq!(sides(&chapters[0].pages[2]), (30, 30));
    }

    #[test]
    fn a_left_to_right_pair_has_the_first_read_page_on_the_left() {
        let mut chapters = book(&[4]);
        join_labelled_spreads(&mut chapters, &[1], false).unwrap();
        assert_eq!(sides(&chapters[0].pages[1]), (10, 20));
    }

    #[test]
    fn positions_count_over_the_whole_book_and_chapters_are_kept() {
        let mut chapters = book(&[2, 3]);
        // Position 3 is the second page of the second chapter.
        let outcome = join_labelled_spreads(&mut chapters, &[3], false).unwrap();
        assert_eq!(outcome.joined, [3]);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].pages.len(), 2);
        assert_eq!(chapters[1].pages.len(), 2);
        assert_eq!(chapters[1].title, "Chapter 1");
        assert_eq!(sides(&chapters[1].pages[1]), (30, 40));
    }

    #[test]
    fn several_pairs_are_joined_at_the_positions_the_list_was_written_for() {
        let mut chapters = book(&[8]);
        let outcome = join_labelled_spreads(&mut chapters, &[5, 1], false).unwrap();
        assert_eq!(outcome.joined, [1, 5]);
        let levels: Vec<(u8, u8)> = chapters[0].pages.iter().map(sides).collect();
        assert_eq!(
            levels,
            [(0, 0), (10, 20), (30, 30), (40, 40), (50, 60), (70, 70)]
        );
    }

    #[test]
    fn a_pair_across_two_chapters_goes_to_the_first_and_an_emptied_chapter_is_dropped() {
        let mut chapters = book(&[2, 1, 2]);
        let outcome = join_labelled_spreads(&mut chapters, &[1], false).unwrap();
        assert_eq!(outcome.joined, [1]);
        let titles: Vec<&str> = chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Chapter 0", "Chapter 2"]);
        assert_eq!(sides(&chapters[0].pages[1]), (10, 20));
    }

    #[test]
    fn positions_that_cannot_be_used_are_skipped_and_reported() {
        let mut chapters = book(&[5]);
        // 4 is the last page; 9 is past the end; 2 is the second half of
        // the pair at 1; 1 is listed twice.
        let outcome = join_labelled_spreads(&mut chapters, &[1, 2, 4, 9, 1], false).unwrap();
        assert_eq!(outcome.joined, [1]);
        assert_eq!(
            outcome.skipped,
            [
                (2, Skipped::PartOfPreviousPair),
                (4, Skipped::NoPageAfter),
                (9, Skipped::NoPageAfter)
            ]
        );
        assert_eq!(chapters[0].pages.len(), 4);
    }

    #[test]
    fn pairs_two_apart_are_both_joined() {
        // 0 and 2 share no page; only a position right after a joined one
        // is its second half.
        let mut chapters = book(&[4]);
        let outcome = join_labelled_spreads(&mut chapters, &[0, 2], false).unwrap();
        assert_eq!(outcome.joined, [0, 2]);
        assert!(outcome.skipped.is_empty());
        assert_eq!(chapters[0].pages.len(), 2);
    }

    #[test]
    fn pages_of_different_sizes_are_both_kept_whole() {
        let mut chapters = vec![Chapter {
            relative_path: PathBuf::new(),
            title: String::new(),
            pages: vec![page((10, 30), 200), page((14, 20), 100)],
        }];
        join_labelled_spreads(&mut chapters, &[0], false).unwrap();
        let joined = decode(&chapters[0].pages[0]);
        assert_eq!(joined.dimensions(), (24, 30));
        assert_eq!(
            joined.get_pixel(9, 29)[0],
            200,
            "the taller left page, whole"
        );
        assert_eq!(
            joined.get_pixel(10, 0)[0],
            100,
            "the right page starts where the left ends"
        );
        assert_eq!(joined.get_pixel(23, 19)[0], 100);
        assert_eq!(
            joined.get_pixel(23, 29)[0],
            0,
            "black below the shorter page"
        );
    }

    #[test]
    fn a_source_page_can_be_followed_to_where_it_ended_up() {
        let outcome = Joined {
            joined: vec![1, 5],
            skipped: Vec::new(),
        };
        let after: Vec<usize> = (0..8).map(|p| outcome.position_after(p)).collect();
        assert_eq!(after, [0, 1, 1, 2, 3, 4, 4, 5]);
    }

    #[test]
    fn a_page_that_is_not_an_image_is_an_error() {
        let mut chapters = book(&[2]);
        chapters[0].pages[1].bytes = b"not an image".to_vec();
        assert!(join_labelled_spreads(&mut chapters, &[0], false).is_err());
    }
}
