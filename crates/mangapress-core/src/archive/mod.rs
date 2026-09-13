//! Archive input/output. v1 scope is `.cbz` (a plain ZIP) and bare folders
//! of images only — matching the actual Mangabind->mangapress contract
//! (see `docs/adr/0005-mangabind-contract.md`). CBR/7z/RAR input is
//! explicitly out of scope for now: KCC itself hard-requires the external
//! `7z` binary for those with no pure-Python fallback, and the `zip` crate
//! gives us a pure-Rust path for the one format that actually matters here.

pub mod cbz;
pub mod folder;

/// One extracted source page, in tree order, with its path relative to the
/// archive/folder root — the relative path is what chapter attribution
/// keys off of (see [`crate::ebook::epub`] and the ADR on the Mangabind
/// contract's basename-collision fix).
pub struct SourceEntry {
    pub relative_path: std::path::PathBuf,
    pub bytes: Vec<u8>,
}

/// Recognized page image extensions (case-insensitive) — matches KCC's own
/// `removeNonImages()` filtering by extension, not by sniffing file content.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp"];

/// Drops entries that aren't recognized page images before they can reach
/// chapter grouping or the image pipeline — without this, a stray `.txt`, a
/// `ComicInfo.xml` nested in a chapter subfolder, or a `.cbz` zipped on
/// macOS (which adds a parallel `__MACOSX/` tree of AppleDouble sidecar
/// files) aborts the whole run when the pipeline tries to decode one of
/// them as an image. macOS's sidecar files keep the real file's own
/// extension (`__MACOSX/._page1.jpg`), so both a path-prefix check and a
/// `._`-filename check are needed alongside the extension allowlist.
///
/// Returns the kept entries and how many were skipped, so the caller can
/// tell the user rather than silently dropping files.
pub fn filter_image_entries(entries: Vec<SourceEntry>) -> (Vec<SourceEntry>, usize) {
    let mut skipped = 0;
    let kept = entries
        .into_iter()
        .filter(|entry| {
            let is_macos_sidecar = entry
                .relative_path
                .components()
                .any(|c| c.as_os_str() == "__MACOSX")
                || entry
                    .relative_path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("._"));
            let is_recognized_image = entry.relative_path.extension().is_some_and(|e| {
                IMAGE_EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str())
            });

            let keep = is_recognized_image && !is_macos_sidecar;
            if !keep {
                skipped += 1;
            }
            keep
        })
        .collect();
    (kept, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(path: &str) -> SourceEntry {
        SourceEntry {
            relative_path: PathBuf::from(path),
            bytes: Vec::new(),
        }
    }

    #[test]
    fn keeps_recognized_image_extensions_case_insensitively() {
        let (kept, skipped) = filter_image_entries(vec![
            entry("c001/p0001.jpg"),
            entry("c001/p0002.JPEG"),
            entry("c001/p0003.png"),
        ]);
        assert_eq!(kept.len(), 3);
        assert_eq!(skipped, 0);
    }

    #[test]
    fn drops_non_image_files() {
        let (kept, skipped) = filter_image_entries(vec![
            entry("c001/p0001.jpg"),
            entry("notes.txt"),
            entry("c001/ComicInfo.xml"),
            entry("Thumbs.db"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(skipped, 3);
    }

    #[test]
    fn drops_macos_resource_fork_sidecars() {
        let (kept, skipped) = filter_image_entries(vec![
            entry("c001/p0001.jpg"),
            entry("__MACOSX/c001/._p0001.jpg"),
            entry("c001/._p0002.jpg"),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(skipped, 2);
    }
}
