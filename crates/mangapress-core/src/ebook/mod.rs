//! Output format builders. No PDF-rendering library dependency is needed
//! anywhere here — mangapress's scope is images/`.cbz` *in*, never PDF in,
//! so [`pdf`] only ever composes already-processed raster pages onto PDF
//! pages (`printpdf`), unlike upstream KCC which also uses PyMuPDF to
//! *read* PDF input.
//!
//! MOBI/AZW3 is permanently out of scope — see
//! `docs/adr/0008-mobi-azw3-permanently-out-of-scope.md`.

pub mod cbz_out;
pub mod epub;
pub mod pdf;

use crate::archive::SourceEntry;
use std::path::PathBuf;

/// One already-processed page: raw encoded image bytes plus the format
/// (needed for the EPUB manifest's media-type and the file extension).
pub struct Page {
    /// Lowercase, no leading dot (e.g. `"jpg"`, `"png"`).
    pub extension: String,
    pub bytes: Vec<u8>,
}

/// One chapter's worth of pages, keyed by its *full relative path* from the
/// archive root (not just its basename — see
/// `docs/adr/0005-mangabind-contract.md` for why upstream KCC's
/// basename-only keying is a bug we're deliberately not inheriting).
pub struct Chapter {
    pub relative_path: PathBuf,
    pub title: String,
    pub pages: Vec<Page>,
}

/// Groups already-extracted [`SourceEntry`] values into [`Chapter`]s by
/// parent directory — reproducing upstream's `buildEPUB()` behavior of
/// treating every directory that contains qualifying files as its own
/// chapter (see the ADR above for the full reasoning, including the one
/// deliberate divergence: chapters are keyed/titled by full path here, not
/// by directory basename alone).
///
/// Every entry sharing the same parent directory lands in the same
/// [`Chapter`], regardless of where else in `entries` that parent
/// reappears — a recursive directory walk (or any input not perfectly
/// grouped by adjacency already) can otherwise interleave a chapter's own
/// entries with a nested subfolder's, which a simpler "same as the
/// previous entry's parent" check would see as two separate chapters
/// sharing one directory. A chapter's position in the output follows where
/// its *first* entry appeared, matching natural-sort order for input that
/// is already contiguous per directory (the common case).
pub fn group_into_chapters(entries: Vec<SourceEntry>) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut chapter_index_by_parent: std::collections::HashMap<PathBuf, usize> =
        std::collections::HashMap::new();

    for entry in entries {
        let parent = entry
            .relative_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let extension = entry
            .relative_path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let page = Page {
            extension,
            bytes: entry.bytes,
        };

        let chapter_index = *chapter_index_by_parent
            .entry(parent.clone())
            .or_insert_with(|| {
                let title = parent
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".to_string());
                chapters.push(Chapter {
                    relative_path: parent,
                    title,
                    pages: Vec::new(),
                });
                chapters.len() - 1
            });

        chapters[chapter_index].pages.push(page);
    }

    chapters
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn entry(path: &str, bytes: &[u8]) -> SourceEntry {
        SourceEntry {
            relative_path: PathBuf::from(path),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn groups_entries_by_parent_directory() {
        let entries = vec![
            entry("c001 - Title One/p0001.jpg", b"a"),
            entry("c001 - Title One/p0002.png", b"b"),
            entry("c002 - Title Two/p0001.jpg", b"c"),
        ];
        let chapters = group_into_chapters(entries);
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "c001 - Title One");
        assert_eq!(chapters[0].pages.len(), 2);
        assert_eq!(chapters[0].pages[0].extension, "jpg");
        assert_eq!(chapters[0].pages[1].extension, "png");
        assert_eq!(chapters[1].title, "c002 - Title Two");
        assert_eq!(chapters[1].pages.len(), 1);
    }

    #[test]
    fn reunites_a_chapters_entries_even_when_a_nested_subfolder_interleaves_them() {
        // Recursive input can legitimately produce this order: a file
        // directly in "a", then a file in "a"'s own subfolder "a/b", then
        // another file directly in "a" again. All the direct-in-"a" pages
        // must land in one chapter, not be split across two.
        let entries = vec![
            entry("a/a.jpg", b"1"),
            entry("a/b/p.jpg", b"2"),
            entry("a/c.jpg", b"3"),
        ];
        let chapters = group_into_chapters(entries);
        assert_eq!(
            chapters.len(),
            2,
            "expected one chapter for \"a\", one for \"a/b\""
        );
        let a_chapter = chapters
            .iter()
            .find(|c| c.relative_path == Path::new("a"))
            .expect("a chapter for \"a\" should exist");
        assert_eq!(
            a_chapter.pages.len(),
            2,
            "both of \"a\"'s own pages should be in the same chapter"
        );
    }

    #[test]
    fn distinguishes_same_named_directories_at_different_paths() {
        // Two different "Extras" folders under different parents must NOT
        // be merged into one chapter — this is the bug fix from ADR 0005.
        let entries = vec![
            entry("VolumeA/Extras/p0001.jpg", b"a"),
            entry("VolumeB/Extras/p0001.jpg", b"b"),
        ];
        let chapters = group_into_chapters(entries);
        assert_eq!(chapters.len(), 2);
        assert_ne!(chapters[0].relative_path, chapters[1].relative_path);
    }
}
