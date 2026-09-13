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

/// Groups already-extracted, naturally-sorted [`SourceEntry`] values into
/// [`Chapter`]s by parent directory — reproducing upstream's `buildEPUB()`
/// behavior of treating every directory that contains qualifying files as
/// its own chapter (see the ADR above for the full reasoning, including the
/// one deliberate divergence: chapters are keyed/titled by full path here,
/// not by directory basename alone).
///
/// `entries` must already be in the order [`crate::archive::cbz::extract_cbz`]
/// produces (natural sort, chapter-directory-first) — this function does not
/// re-sort, it only groups adjacent entries that share a parent directory.
pub fn group_into_chapters(entries: Vec<SourceEntry>) -> Vec<Chapter> {
    let mut chapters: Vec<Chapter> = Vec::new();

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

        let starts_new_chapter = chapters
            .last()
            .is_none_or(|c: &Chapter| c.relative_path != parent);

        if starts_new_chapter {
            let title = parent
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_string());
            chapters.push(Chapter {
                relative_path: parent,
                title,
                pages: Vec::new(),
            });
        }

        chapters.last_mut().unwrap().pages.push(page);
    }

    chapters
}

#[cfg(test)]
mod tests {
    use super::*;

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
