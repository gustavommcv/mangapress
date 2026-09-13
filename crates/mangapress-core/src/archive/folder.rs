//! Reading a bare folder of chapter subfolders/images as if it were an
//! already-extracted archive — the other half of the "`.cbz` or a folder"
//! input contract (see [`super`]'s module docs).

use super::SourceEntry;
use crate::error::Result;
use std::path::Path;

/// Recursively read every file under `root`, with paths relative to `root`
/// itself — mirrors [`super::cbz::extract_from_reader`]'s contract exactly
/// (same [`SourceEntry`] shape, same natural-sort ordering), so callers can
/// treat a `.cbz` and a bare folder identically from this point on.
pub fn read_folder(root: &Path) -> Result<Vec<SourceEntry>> {
    let mut entries = Vec::new();
    collect(root, root, &mut entries)?;
    entries.sort_by(|a, b| crate::natural_sort::compare_paths(&a.relative_path, &b.relative_path));
    Ok(entries)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<SourceEntry>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let relative_path = path
                .strip_prefix(root)
                .expect("path was read from under root")
                .to_path_buf();
            let bytes = std::fs::read(&path)?;
            out.push(SourceEntry {
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
