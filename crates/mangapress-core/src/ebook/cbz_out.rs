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

pub fn build_cbz(chapters: &[Chapter]) -> Result<Vec<u8>> {
    let mut entries = Vec::new();

    for chapter in chapters {
        let safe_title = chapter.title.replace(['/', '\\'], "-");
        for (page_index, page) in chapter.pages.iter().enumerate() {
            let name = format!("{safe_title}/p{:04}.{}", page_index + 1, page.extension);
            entries.push((name, page.bytes.clone()));
        }
    }

    if entries.is_empty() {
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
                })
                .collect(),
        }
    }

    #[test]
    fn empty_chapters_are_rejected() {
        assert!(matches!(build_cbz(&[]), Err(Error::EmptyBook)));
    }

    #[test]
    fn builds_one_folder_per_chapter() {
        let chapters = vec![chapter("c001 - One", 2), chapter("c002 - Two", 1)];
        let bytes = build_cbz(&chapters).unwrap();
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
    fn slashes_in_titles_are_sanitized() {
        let chapters = vec![chapter("c001 - A/B", 1)];
        let bytes = build_cbz(&chapters).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "c001 - A-B/p0001.jpg");
    }
}
