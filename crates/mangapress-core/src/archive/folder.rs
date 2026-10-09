//! Reading a bare folder of chapter subfolders/images as if it were an
//! already-extracted archive — the other half of the "`.cbz` or a folder"
//! input contract (see [`super`]'s module docs).

use super::{BookInput, LinkSkipReason, SkippedLink, SourceEntry};
use crate::error::Result;
use std::path::{Path, PathBuf};

/// Recursively read every file under `root`, with paths relative to `root`
/// itself — mirrors [`super::cbz::extract_from_reader`]'s contract exactly
/// (same [`SourceEntry`] shape, same natural-sort ordering), so callers can
/// treat a `.cbz` and a bare folder identically from this point on.
/// Only links to regular files inside the input are followed (ADR 0017).
/// Use [`super::read_book`] for page selection and skipped-link diagnostics.
pub fn read_folder(root: &Path) -> Result<Vec<SourceEntry>> {
    Ok(read_selected(root, |_| true)?.entries)
}

pub(super) fn read_selected(root: &Path, keep: impl Fn(&Path) -> bool) -> Result<BookInput> {
    // The explicitly selected root may itself be a link. Descendant links
    // are compared to its real path, without changing their names in the book.
    let real_root = std::fs::canonicalize(root)?;
    let mut input = BookInput::default();
    collect(root, &real_root, root, &keep, &mut input)?;
    input
        .entries
        .sort_by(|a, b| crate::natural_sort::compare_paths(&a.relative_path, &b.relative_path));
    input
        .skipped_links
        .sort_by(|a, b| crate::natural_sort::compare_paths(&a.relative_path, &b.relative_path));
    Ok(input)
}

fn collect(
    root: &Path,
    real_root: &Path,
    dir: &Path,
    keep: &impl Fn(&Path) -> bool,
    out: &mut BookInput,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        // DirEntry::file_type does not follow links, unlike Path::is_dir.
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect(root, real_root, &path, keep, out)?;
        } else {
            let relative_path = path
                .strip_prefix(root)
                .expect("path was read from under root")
                .to_path_buf();
            let read_path = if file_type.is_symlink() {
                match resolve_file_link(real_root, &path) {
                    Ok(target) => target,
                    Err(reason) => {
                        out.skipped_links.push(SkippedLink {
                            relative_path,
                            reason,
                        });
                        continue;
                    }
                }
            } else {
                path
            };
            if !keep(&relative_path) {
                out.skipped_non_images += 1;
                continue;
            }
            let bytes = crate::input::read_file(&read_path)?;
            out.entries.push(SourceEntry {
                relative_path,
                bytes,
            });
        }
    }
    Ok(())
}

fn resolve_file_link(
    real_root: &Path,
    path: &Path,
) -> std::result::Result<PathBuf, LinkSkipReason> {
    let target = std::fs::canonicalize(path).map_err(|_| LinkSkipReason::Unresolved)?;
    // Component comparison, not a string prefix: root-other is not inside root.
    if !target.starts_with(real_root) {
        return Err(LinkSkipReason::OutsideInput);
    }
    if !std::fs::metadata(&target)
        .map_err(|_| LinkSkipReason::Unresolved)?
        .is_file()
    {
        return Err(LinkSkipReason::NotAFile);
    }
    // Open the checked target rather than following the original link again.
    // This is not a sandbox against a tree being changed concurrently.
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(unix, windows))]
    mod links {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/folder_links.rs"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_junctions_are_skipped_without_recursing_or_reading_external_files() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("book");
        let outside = parent.path().join("book-other");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("p1.png"), b"page").unwrap();
        std::fs::write(outside.join("private.png"), b"private image").unwrap();
        links::junction(&root, &root.join("loop"));
        links::junction(&outside, &root.join("outside"));
        let input = super::super::read_book(&root).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.entries[0].bytes, b"page");
        assert_eq!(
            input.skipped_links,
            [
                SkippedLink {
                    relative_path: PathBuf::from("loop"),
                    reason: LinkSkipReason::NotAFile
                },
                SkippedLink {
                    relative_path: PathBuf::from("outside"),
                    reason: LinkSkipReason::OutsideInput
                },
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn an_explicitly_selected_windows_junction_can_be_the_input_root() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("book");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("p1.png"), b"page").unwrap();
        let shortcut = parent.path().join("shortcut");
        links::junction(&root, &shortcut);
        let input = super::super::read_book(&shortcut).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.entries[0].relative_path, Path::new("p1.png"));
        assert_eq!(input.entries[0].bytes, b"page");
        assert!(input.skipped_links.is_empty());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn internal_absolute_relative_and_chained_file_links_keep_their_names() {
        let root = tempfile::tempdir().unwrap();
        if !links::supported(root.path()) {
            return;
        }
        let chapter = root.path().join("c001");
        std::fs::create_dir(&chapter).unwrap();
        let source = chapter.join("p1.png");
        std::fs::write(&source, b"page").unwrap();
        links::file_link(Path::new("p1.png"), &chapter.join("p2.png")).unwrap();
        links::file_link(&chapter.join("p2.png"), &chapter.join("p3.png")).unwrap();
        links::file_link(&source, &chapter.join("p10.png")).unwrap();
        let input = super::super::read_book(root.path()).unwrap();
        assert!(input.skipped_links.is_empty());
        let names: Vec<_> = input
            .entries
            .iter()
            .map(|entry| entry.relative_path.clone())
            .collect();
        assert_eq!(
            names,
            ["c001/p1.png", "c001/p2.png", "c001/p3.png", "c001/p10.png"].map(PathBuf::from)
        );
        assert!(input.entries.iter().all(|entry| entry.bytes == b"page"));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn external_targets_are_skipped_through_chains_and_directory_hops() {
        let parent = tempfile::tempdir().unwrap();
        if !links::supported(parent.path()) {
            return;
        }
        let root = parent.path().join("book");
        let outside = parent.path().join("book-other");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("p1.png"), b"page").unwrap();
        let private = outside.join("private.png");
        std::fs::write(&private, b"private image").unwrap();
        links::file_link(&private, &root.join("a.png")).unwrap();
        // Windows preserves the symlink target string. Build native separators
        // rather than accidentally creating a broken POSIX-style relative link.
        let relative_target = Path::new("..").join("book-other").join("private.png");
        links::file_link(&relative_target, &root.join("b.png")).unwrap();
        links::file_link(&root.join("a.png"), &root.join("c.png")).unwrap();
        links::directory_link(&outside, &root.join("door")).unwrap();
        links::file_link(&root.join("door").join("private.png"), &root.join("d.png")).unwrap();
        let expected_target = std::fs::canonicalize(&private).unwrap();
        for name in ["a.png", "b.png", "c.png", "d.png"] {
            assert_eq!(
                std::fs::canonicalize(root.join(name)).unwrap(),
                expected_target,
                "{name} must be a genuine external-file fixture"
            );
        }
        let input = super::super::read_book(&root).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.entries[0].bytes, b"page");
        assert_eq!(input.skipped_non_images, 0);
        let names: Vec<_> = input
            .skipped_links
            .iter()
            .map(|link| link.relative_path.clone())
            .collect();
        assert_eq!(
            names,
            ["a.png", "b.png", "c.png", "d.png", "door"].map(PathBuf::from)
        );
        assert!(
            input
                .skipped_links
                .iter()
                .all(|link| link.reason == LinkSkipReason::OutsideInput),
            "{:?}",
            input.skipped_links
        );
        assert_eq!(
            read_folder(&root).unwrap().len(),
            1,
            "the generic reader applies the same policy"
        );
        assert_eq!(std::fs::read(&private).unwrap(), b"private image");
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn a_file_link_through_an_internal_directory_link_is_kept_without_traversing_that_directory() {
        let root = tempfile::tempdir().unwrap();
        if !links::supported(root.path()) {
            return;
        }
        let chapter = root.path().join("c001");
        std::fs::create_dir(&chapter).unwrap();
        std::fs::write(chapter.join("p1.png"), b"page").unwrap();
        let alias = root.path().join("directory-alias");
        links::directory_link(&chapter, &alias).unwrap();
        links::file_link(&alias.join("p1.png"), &root.path().join("p2.png")).unwrap();
        let input = super::super::read_book(root.path()).unwrap();
        assert_eq!(input.entries.len(), 2);
        // The file beside the chapter folder comes first.
        assert_eq!(input.entries[0].relative_path, Path::new("p2.png"));
        assert_eq!(input.entries[1].relative_path, Path::new("c001/p1.png"));
        assert!(input.entries.iter().all(|entry| entry.bytes == b"page"));
        assert_eq!(
            input.skipped_links,
            [SkippedLink {
                relative_path: PathBuf::from("directory-alias"),
                reason: LinkSkipReason::NotAFile,
            }]
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn broken_cyclic_and_directory_links_are_skipped_without_recursing() {
        let root = tempfile::tempdir().unwrap();
        if !links::supported(root.path()) {
            return;
        }
        let empty = root.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        std::fs::write(root.path().join("p1.png"), b"page").unwrap();
        links::file_link(Path::new("missing.png"), &root.path().join("a.png")).unwrap();
        links::file_link(Path::new("c.png"), &root.path().join("b.png")).unwrap();
        links::file_link(Path::new("b.png"), &root.path().join("c.png")).unwrap();
        links::directory_link(&empty, &root.path().join("dir-link")).unwrap();
        links::directory_link(root.path(), &empty.join("parent")).unwrap();
        let input = super::super::read_book(root.path()).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.skipped_links.len(), 5);
        assert_eq!(
            input
                .skipped_links
                .iter()
                .filter(|link| link.reason == LinkSkipReason::Unresolved)
                .count(),
            3
        );
        assert_eq!(
            input
                .skipped_links
                .iter()
                .filter(|link| link.reason == LinkSkipReason::NotAFile)
                .count(),
            2
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn an_explicitly_selected_root_link_uses_its_real_containment_boundary() {
        let parent = tempfile::tempdir().unwrap();
        if !links::supported(parent.path()) {
            return;
        }
        let root = parent.path().join("book");
        std::fs::create_dir(&root).unwrap();
        let page = root.join("p1.png");
        std::fs::write(&page, b"page").unwrap();
        let private = parent.path().join("private.png");
        std::fs::write(&private, b"private image").unwrap();
        links::file_link(&page, &root.join("p2.png")).unwrap();
        links::file_link(&private, &root.join("p3.png")).unwrap();
        let shortcut = parent.path().join("shortcut");
        links::directory_link(&root, &shortcut).unwrap();
        let input = super::super::read_book(&shortcut).unwrap();
        assert_eq!(input.entries.len(), 2);
        assert_eq!(
            input.skipped_links,
            [SkippedLink {
                relative_path: PathBuf::from("p3.png"),
                reason: LinkSkipReason::OutsideInput
            }]
        );
        assert!(input.entries.iter().all(|entry| entry.bytes == b"page"));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn metadata_links_have_the_same_boundary_and_ignored_files_stay_filtered() {
        let parent = tempfile::tempdir().unwrap();
        if !links::supported(parent.path()) {
            return;
        }
        let root = parent.path().join("book");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("p1.png"), b"page").unwrap();
        let metadata = root.join("metadata.txt");
        std::fs::write(&metadata, b"internal metadata").unwrap();
        let link = root.join("ComicInfo.xml");
        links::file_link(&metadata, &link).unwrap();
        links::file_link(&metadata, &root.join("notes.txt")).unwrap();
        let input = super::super::read_book(&root).unwrap();
        assert_eq!(input.entries.len(), 2);
        assert_eq!(input.skipped_non_images, 2);
        assert!(input.skipped_links.is_empty());
        assert_eq!(
            input
                .entries
                .iter()
                .find(|entry| entry.relative_path == Path::new("ComicInfo.xml"))
                .unwrap()
                .bytes,
            b"internal metadata"
        );
        std::fs::remove_file(&link).unwrap();
        let private = parent.path().join("private.xml");
        std::fs::write(&private, b"external metadata").unwrap();
        links::file_link(&private, &link).unwrap();
        let input = super::super::read_book(&root).unwrap();
        assert_eq!(input.entries.len(), 1);
        assert_eq!(input.skipped_non_images, 2);
        assert_eq!(
            input.skipped_links,
            [SkippedLink {
                relative_path: PathBuf::from("ComicInfo.xml"),
                reason: LinkSkipReason::OutsideInput
            }]
        );
    }

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
        assert_eq!(input.entries[0].bytes, b"metadata");
        assert_eq!(input.entries[1].relative_path, Path::new("c001/p1.PNG"));
        assert_eq!(input.entries[1].bytes, b"page");
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
