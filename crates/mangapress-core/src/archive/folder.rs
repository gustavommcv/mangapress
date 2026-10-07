//! Reading a bare folder of chapter subfolders/images as if it were an
//! already-extracted archive — the other half of the "`.cbz` or a folder"
//! input contract (see [`super`]'s module docs).

use super::{BookInput, SourceEntry};
use crate::error::Result;
use std::path::Path;

/// Recursively read every file under `root`, with paths relative to `root`
/// itself — mirrors [`super::cbz::extract_from_reader`]'s contract exactly
/// (same [`SourceEntry`] shape, same natural-sort ordering), so callers can
/// treat a `.cbz` and a bare folder identically from this point on.
pub fn read_folder(root: &Path) -> Result<Vec<SourceEntry>> {
    Ok(read_selected(root, |_| true)?.entries)
}

pub(super) fn read_selected(root: &Path, keep: impl Fn(&Path) -> bool) -> Result<BookInput> {
    let mut input = BookInput::default();
    collect(root, root, &keep, &mut input)?;
    input
        .entries
        .sort_by(|a, b| crate::natural_sort::compare_paths(&a.relative_path, &b.relative_path));
    Ok(input)
}

fn collect(
    root: &Path,
    dir: &Path,
    keep: &impl Fn(&Path) -> bool,
    out: &mut BookInput,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, keep, out)?;
        } else {
            let relative_path = path
                .strip_prefix(root)
                .expect("path was read from under root")
                .to_path_buf();
            if !keep(&relative_path) {
                out.skipped_non_images += 1;
                continue;
            }
            let bytes = crate::input::read_file(&path)?;
            out.entries.push(SourceEntry {
                relative_path,
                bytes,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_selection_preserves_root_metadata_and_counts_ignored_files() {
        let root = tempfile::tempdir().unwrap();
        let chapter = root.path().join("c001");
        std::fs::create_dir(&chapter).unwrap();
        std::fs::write(root.path().join("ComicInfo.xml"), b"metadata").unwrap();
        std::fs::write(chapter.join("p1.PNG"), b"page").unwrap();
        std::fs::write(chapter.join("ComicInfo.xml"), b"nested metadata").unwrap();
        std::fs::write(chapter.join("._p2.png"), b"sidecar").unwrap();
        std::fs::write(root.path().join("notes.txt"), b"notes").unwrap();
        let input = super::super::read_book(root.path()).unwrap();
        assert_eq!(input.skipped_non_images, 3);
        assert_eq!(input.entries.len(), 2);
        assert_eq!(input.entries[0].relative_path, Path::new("c001/p1.PNG"));
        assert_eq!(input.entries[0].bytes, b"page");
        assert_eq!(input.entries[1].bytes, b"metadata");
        assert_eq!(
            read_folder(root.path()).unwrap().len(),
            5,
            "the generic reader retains its contract"
        );
    }

    #[cfg(windows)]
    #[test]
    fn ignored_files_are_not_opened_even_when_another_process_locks_them() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("p1.png"), b"page").unwrap();
        let notes = root.path().join("notes.txt");
        std::fs::write(&notes, b"notes").unwrap();
        let _lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(notes)
            .unwrap();
        let input = super::super::read_book(root.path()).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.skipped_non_images, 1);
    }

    #[test]
    fn reads_nested_files_with_paths_relative_to_root_naturally_sorted() {
        let root =
            std::env::temp_dir().join(format!("mangapress-folder-test-{}", std::process::id()));
        let chapter_two = root.join("c002 - Two");
        let chapter_one = root.join("c001 - One");
        std::fs::create_dir_all(&chapter_one).unwrap();
        std::fs::create_dir_all(&chapter_two).unwrap();
        std::fs::write(chapter_one.join("p0002.jpg"), b"one-two").unwrap();
        std::fs::write(chapter_one.join("p0001.jpg"), b"one-one").unwrap();
        std::fs::write(chapter_two.join("p0001.jpg"), b"two-one").unwrap();

        let entries = read_folder(&root).unwrap();
        std::fs::remove_dir_all(&root).ok();

        let names: Vec<String> = entries
            .iter()
            .map(|e| e.relative_path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            vec![
                "c001 - One/p0001.jpg",
                "c001 - One/p0002.jpg",
                "c002 - Two/p0001.jpg",
            ]
        );
        assert_eq!(entries[0].bytes, b"one-one");
    }
}
