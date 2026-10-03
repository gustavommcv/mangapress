//! Resized-`.cbz` output: the processed pages re-zipped under their
//! original chapter titles (sanitized only enough to be safe zip entry
//! names — `/` would otherwise be read back as a spurious subdirectory).
//!
//! Unlike [`super::epub`], there's no TOC to build, so chapter titles go
//! straight into the archive's own folder names instead of staying purely
//! a display label — a resized CBZ is meant to be human-browsable the same
//! way the source `.cbz` was.

use super::Chapter;
use crate::error::{Error, Result};

/// `--keepcomicinfo`: re-embeds the *original* `ComicInfo.xml` bytes
/// (unmodified — upstream doesn't rewrite it to reflect the resize either)
/// at the root of the output CBZ. `None` when the source had no
/// `ComicInfo.xml` or `--keepcomicinfo` wasn't requested.
///
/// `cover`, when given, goes in as `##cover.jpg` at the root — upstream's
/// name for it, chosen to sort ahead of every page so that readers show it
/// first. Upstream only adds one when the cover is not simply the first
/// page: a cover the user supplied, or one smart-cropped out of a wide
/// image.
pub fn build_cbz(
    chapters: &[Chapter],
    comic_info_xml: Option<&[u8]>,
    cover: Option<&[u8]>,
) -> Result<Vec<u8>> {
    let mut entries = Vec::new();

    if let Some(cover) = cover {
        entries.push(("##cover.jpg".to_string(), cover.to_vec()));
    }

    if let Some(xml) = comic_info_xml {
        entries.push(("ComicInfo.xml".to_string(), xml.to_vec()));
    }

    // Chapter titles are only the parent directory's basename (see
    // `super::group_into_chapters`), so two chapters at different paths can
    // legitimately share one (e.g. "VolumeA/Extras" and "VolumeB/Extras").
    // The first chapter with a given title keeps it unchanged -- this is
    // the overwhelmingly common case and matches every existing fixture --
    // later duplicates get a disambiguating suffix instead of silently
    // colliding into the same zip directory.
    let mut seen_title_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for chapter in chapters {
        let base_title = chapter.title.replace(['/', '\\'], "-");
        let occurrence = seen_title_counts.entry(base_title.clone()).or_insert(0);
        *occurrence += 1;
        let safe_title = if *occurrence == 1 {
            base_title
        } else {
            format!("{base_title} ({occurrence})")
        };
        for (page_index, page) in chapter.pages.iter().enumerate() {
            let name = format!("{safe_title}/p{:04}.{}", page_index + 1, page.extension);
            entries.push((name, page.bytes.clone()));
        }
    }

    let extras = comic_info_xml.is_some() as usize + cover.is_some() as usize;
    if entries.len() == extras {
        return Err(Error::EmptyBook);
    }

    crate::archive::cbz::write_zip(&entries, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ebook::Page;
    use std::path::PathBuf;

    fn chapter(title: &str, pages: usize) -> Chapter {
        Chapter {
            relative_path: PathBuf::from(title),
            title: title.to_string(),
            pages: (0..pages)
                .map(|_| Page {
                    extension: "jpg".to_string(),
                    bytes: b"fake-jpeg".to_vec(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    #[test]
    fn empty_chapters_are_rejected() {
        assert!(matches!(build_cbz(&[], None, None), Err(Error::EmptyBook)));
    }

    #[test]
    fn empty_chapters_with_comicinfo_are_still_rejected() {
        // A lone ComicInfo.xml with no actual pages isn't a book.
        assert!(matches!(
            build_cbz(&[], Some(b"<ComicInfo/>"), None),
            Err(Error::EmptyBook)
        ));
    }

    #[test]
    fn builds_one_folder_per_chapter() {
        let chapters = vec![chapter("c001 - One", 2), chapter("c002 - Two", 1)];
        let bytes = build_cbz(&chapters, None, None).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "c001 - One/p0001.jpg",
                "c001 - One/p0002.jpg",
                "c002 - Two/p0001.jpg",
            ]
        );
    }

    #[test]
    fn duplicate_chapter_titles_get_disambiguated() {
        // "VolumeA/Extras" and "VolumeB/Extras" both title their chapter
        // "Extras" -- the first keeps the plain name, the second must not
        // collide with it in the output zip.
        let chapters = vec![chapter("Extras", 1), chapter("Extras", 1)];
        let bytes = build_cbz(&chapters, None, None).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names, vec!["Extras/p0001.jpg", "Extras (2)/p0001.jpg"]);
    }

    #[test]
    fn slashes_in_titles_are_sanitized() {
        let chapters = vec![chapter("c001 - A/B", 1)];
        let bytes = build_cbz(&chapters, None, None).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "c001 - A-B/p0001.jpg");
    }

    #[test]
    fn keepcomicinfo_embeds_the_original_bytes_at_the_root() {
        let chapters = vec![chapter("c001 - One", 1)];
        let bytes = build_cbz(
            &chapters,
            Some(b"<ComicInfo><Series>Test</Series></ComicInfo>"),
            None,
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "ComicInfo.xml");
        let mut contents = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("ComicInfo.xml").unwrap(),
            &mut contents,
        )
        .unwrap();
        assert_eq!(contents, "<ComicInfo><Series>Test</Series></ComicInfo>");
    }

    #[test]
    fn a_cover_is_stored_under_upstreams_name_ahead_of_every_page() {
        let chapters = vec![Chapter {
            relative_path: PathBuf::from("c001"),
            title: "c001".to_string(),
            pages: vec![Page {
                extension: "jpg".to_string(),
                bytes: vec![1, 2, 3],
                ..Default::default()
            }],
        }];
        let bytes = build_cbz(&chapters, None, Some(b"cover bytes")).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names[0], "##cover.jpg");
        assert_eq!(names.len(), 2);

        // A cover alone is still an empty book.
        assert!(matches!(
            build_cbz(&[], None, Some(b"cover bytes")),
            Err(Error::EmptyBook)
        ));
    }
}
