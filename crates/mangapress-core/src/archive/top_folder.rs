//! The folder a `.cbz` made by zipping a folder carries around its contents.
//!
//! KCC reads an archive whose only top-level entry is one folder as if that folder were not
//! there. Two things follow from it that matter here: pages lying directly in that folder
//! are pages lying directly in the book (their entry in the contents carries the book's
//! title, not the folder's name), and a `ComicInfo.xml` inside the folder is the book's.
//!
//! Only that much is taken over. A folder that holds chapter folders keeps its place in the
//! paths: `--nested-toc` names a volume after it, and the chapters are told apart by their
//! full path, which is what the Mangabind layout (volume folder, chapter folders) relies on.

use super::{is_page_image, BookInput};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const COMIC_INFO: &str = "ComicInfo.xml";

/// A `ComicInfo.xml` exactly one folder down: where a zipped folder keeps its own.
pub(super) fn is_folder_comic_info(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == COMIC_INFO) && path.components().count() == 2
}

/// The one folder every page lies under, if there is exactly one and no page lies beside it.
fn single_top_folder(input: &BookInput) -> Option<OsString> {
    let mut top: Option<OsString> = None;
    for entry in input
        .entries
        .iter()
        .filter(|entry| is_page_image(&entry.relative_path))
    {
        let mut parts = entry.relative_path.components();
        let first = parts.next()?.as_os_str();
        parts.next()?;
        match &top {
            Some(known) if known != first => return None,
            Some(_) => {}
            None => top = Some(first.to_owned()),
        }
    }
    top
}

/// Applies what is described in the module documentation to the entries of a `.cbz`, and
/// counts a `ComicInfo.xml` one folder down that is not the book's among the files ignored.
/// A `ComicInfo.xml` at the root of the archive wins over one inside the folder.
pub(super) fn settle(input: &mut BookInput) {
    let wrapper = single_top_folder(input);
    let has_root_info = input
        .entries
        .iter()
        .any(|entry| entry.relative_path == Path::new(COMIC_INFO));
    let pages_in_it = wrapper.is_some()
        && input.entries.iter().any(|entry| {
            is_page_image(&entry.relative_path) && entry.relative_path.components().count() == 2
        });

    let mut settled = Vec::with_capacity(input.entries.len());
    let mut promoted = has_root_info;
    for mut entry in std::mem::take(&mut input.entries) {
        if is_folder_comic_info(&entry.relative_path) {
            let inside_wrapper = wrapper
                .as_ref()
                .is_some_and(|top| entry.relative_path.starts_with(top));
            if inside_wrapper && !promoted {
                promoted = true;
                entry.relative_path = PathBuf::from(COMIC_INFO);
                settled.push(entry);
            } else {
                input.skipped_non_images += 1;
            }
            continue;
        }
        // Only what lies inside the folder loses it: a `ComicInfo.xml` at the root is not in it.
        let inside = wrapper.as_ref().is_some_and(|top| {
            entry.relative_path.starts_with(top) && entry.relative_path != Path::new(top)
        });
        if pages_in_it && inside {
            entry.relative_path = entry.relative_path.components().skip(1).collect();
        }
        settled.push(entry);
    }
    input.entries = settled;
}

#[cfg(test)]
mod tests {
    use super::super::SourceEntry;
    use super::*;

    fn input(paths: &[&str]) -> BookInput {
        BookInput {
            entries: paths
                .iter()
                .map(|path| SourceEntry {
                    relative_path: PathBuf::from(path),
                    bytes: path.as_bytes().to_vec(),
                })
                .collect(),
            ..BookInput::default()
        }
    }

    fn paths(input: &BookInput) -> Vec<String> {
        input
            .entries
            .iter()
            .map(|entry| entry.relative_path.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn pages_lying_directly_in_the_only_folder_become_pages_of_the_book() {
        let mut book = input(&[
            "My Wrapper/001.png",
            "My Wrapper/002.png",
            "My Wrapper/ComicInfo.xml",
        ]);

        settle(&mut book);

        assert_eq!(paths(&book), ["001.png", "002.png", "ComicInfo.xml"]);
        assert_eq!(book.entries[2].bytes, b"My Wrapper/ComicInfo.xml");
        assert_eq!(book.skipped_non_images, 0);
    }

    #[test]
    fn a_folder_that_holds_only_chapter_folders_keeps_its_place_in_the_paths() {
        let mut book = input(&[
            "v001 - Vol.01/c001 - One/p0001.png",
            "v001 - Vol.01/c002 - Two/p0001.png",
        ]);

        settle(&mut book);

        assert_eq!(
            paths(&book),
            [
                "v001 - Vol.01/c001 - One/p0001.png",
                "v001 - Vol.01/c002 - Two/p0001.png"
            ]
        );
    }

    #[test]
    fn its_comic_info_is_the_books_even_when_the_paths_stay() {
        let mut book = input(&["Series/c001/p0001.png", "Series/ComicInfo.xml"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["Series/c001/p0001.png", "ComicInfo.xml"]);
    }

    #[test]
    fn a_comic_info_at_the_root_wins_and_the_other_one_is_counted_as_ignored() {
        let mut book = input(&["ComicInfo.xml", "Wrapper/001.png", "Wrapper/ComicInfo.xml"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["ComicInfo.xml", "001.png"]);
        assert_eq!(book.entries[0].bytes, b"ComicInfo.xml");
        assert_eq!(book.skipped_non_images, 1);
    }

    #[test]
    fn with_two_folders_nothing_is_a_wrapper_and_a_comic_info_one_folder_down_is_ignored() {
        let mut book = input(&["A/001.png", "A/ComicInfo.xml", "B/001.png"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["A/001.png", "B/001.png"]);
        assert_eq!(book.skipped_non_images, 1);
    }

    #[test]
    fn a_page_beside_the_folder_means_the_folder_is_not_a_wrapper() {
        let mut book = input(&["cover.png", "Chapter 1/001.png", "Chapter 1/ComicInfo.xml"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["cover.png", "Chapter 1/001.png"]);
        assert_eq!(book.skipped_non_images, 1);
    }

    #[test]
    fn pages_lying_in_the_book_itself_stay_where_they_are() {
        let mut book = input(&["001.png", "002.png", "ComicInfo.xml"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["001.png", "002.png", "ComicInfo.xml"]);
        assert_eq!(book.skipped_non_images, 0);
    }

    #[test]
    fn a_folder_with_pages_and_chapter_folders_is_taken_away_whole() {
        let mut book = input(&["W/intro.png", "W/Ch 1/001.png"]);

        settle(&mut book);

        assert_eq!(paths(&book), ["intro.png", "Ch 1/001.png"]);
    }

    #[test]
    fn a_book_with_no_pages_is_left_alone() {
        let mut book = input(&["Wrapper/ComicInfo.xml"]);

        settle(&mut book);

        assert!(paths(&book).is_empty());
        assert_eq!(book.skipped_non_images, 1);
    }

    #[test]
    fn only_a_file_named_comic_info_one_folder_down_is_a_candidate() {
        assert!(is_folder_comic_info(Path::new("Wrapper/ComicInfo.xml")));
        assert!(!is_folder_comic_info(Path::new("ComicInfo.xml")));
        assert!(!is_folder_comic_info(Path::new("A/B/ComicInfo.xml")));
        assert!(!is_folder_comic_info(Path::new("Wrapper/comicinfo.txt")));
    }
}
