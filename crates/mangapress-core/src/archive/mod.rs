//! Archive input/output. The scope is `.cbz` (a plain ZIP) and bare folders
//! of images only — matching the actual Mangabind->mangapress contract
//! (see `docs/adr/0005-mangabind-contract.md`). CBR/7z/RAR input is out of
//! scope on purpose (`docs/adr/0013-follow-a-named-kcc-release.md`): KCC
//! itself hard-requires the external `7z` binary for those with no
//! pure-Python fallback, and the `zip` crate gives us a pure-Rust path for
//! the one format that actually matters here.

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

/// A book's source entries and diagnostics for entries rejected before reading.
#[derive(Default)]
pub struct BookInput {
    pub entries: Vec<SourceEntry>,
    pub skipped_non_images: usize,
    pub skipped_links: Vec<SkippedLink>,
}

/// A rejected folder link. Only its own relative path is retained, not its target.
#[derive(Debug, PartialEq, Eq)]
pub struct SkippedLink {
    pub relative_path: std::path::PathBuf,
    pub reason: LinkSkipReason,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkSkipReason {
    OutsideInput,
    NotAFile,
    Unresolved,
}

impl std::fmt::Display for LinkSkipReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::OutsideInput => "it leads outside the input folder",
            Self::NotAFile => "it does not lead to a regular file",
            Self::Unresolved => "its target could not be resolved",
        })
    }
}

/// Read only page images and root-level `ComicInfo.xml`, preserving natural
/// order and reporting ignored files/links without loading their contents.
pub fn read_book(path: &std::path::Path) -> crate::Result<BookInput> {
    let keep = |entry: &std::path::Path| {
        entry == std::path::Path::new("ComicInfo.xml") || is_page_image(entry)
    };
    if path.is_dir() {
        folder::read_selected(path, keep)
    } else {
        cbz::extract_selected(std::fs::File::open(path)?, keep)
    }
}

/// Recognized page image extensions (case-insensitive) — matches KCC's own
/// `removeNonImages()` filtering by extension, not by sniffing file content.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "webp"];

/// Whether `path` is named like a page image this tool reads.
pub fn has_image_extension(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

fn is_page_image(path: &std::path::Path) -> bool {
    let is_macos_sidecar = path.components().any(|c| c.as_os_str() == "__MACOSX")
        || path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("._"));
    has_image_extension(path) && !is_macos_sidecar
}

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
            let keep = is_page_image(&entry.relative_path);
            if !keep {
                skipped += 1;
            }
            keep
        })
        .collect();
    (kept, skipped)
}

/// How many of the entries are images smaller than `device` on *both*
/// axes, and how many could be measured at all — upstream's
/// `detectSuboptimalProcessing()` count, behind its "more than 25% of images
/// are smaller than the device" warning. Only headers are read.
pub fn smaller_than_device(entries: &[SourceEntry], device: (u32, u32)) -> (usize, usize) {
    let (mut smaller, mut measured) = (0, 0);
    for entry in entries {
        let dimensions = image::ImageReader::new(std::io::Cursor::new(&entry.bytes))
            .with_guessed_format()
            .ok()
            .and_then(|reader| reader.into_dimensions().ok());
        if let Some((width, height)) = dimensions {
            measured += 1;
            if device.0 > width && device.1 > height {
                smaller += 1;
            }
        }
    }
    (smaller, measured)
}

/// Whether the input looks like something KCC already converted: upstream
/// names every page it writes `...-kcc-x`, `-kcc-b` and so on, and warns
/// that converting those again only loses quality.
pub fn looks_already_converted(entries: &[SourceEntry]) -> bool {
    entries.iter().any(|entry| {
        entry
            .relative_path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy().contains("-kcc"))
    })
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

    fn png_entry(name: &str, width: u32, height: u32) -> SourceEntry {
        let mut bytes = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::new(width, height))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        SourceEntry {
            relative_path: std::path::PathBuf::from(name),
            bytes,
        }
    }

    #[test]
    fn counts_images_smaller_than_the_device_on_both_axes() {
        let entries = vec![
            png_entry("c1/a.png", 600, 900),   // smaller both ways
            png_entry("c1/b.png", 1072, 900),  // as wide as the device: not smaller
            png_entry("c1/c.png", 2000, 3000), // larger
            SourceEntry {
                relative_path: std::path::PathBuf::from("c1/broken.png"),
                bytes: b"not an image".to_vec(),
            },
        ];
        assert_eq!(smaller_than_device(&entries, (1072, 1448)), (1, 3));
    }

    #[test]
    fn recognizes_pages_kcc_already_wrote() {
        assert!(looks_already_converted(&[png_entry(
            "c1/kcc-0001-kcc-x.png",
            4,
            4
        )]));
        assert!(!looks_already_converted(&[png_entry("c1/p0001.png", 4, 4)]));
    }
}
