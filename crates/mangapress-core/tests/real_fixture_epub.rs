//! Validates the Mangabind contract end-to-end against a real,
//! Mangabind-produced `.cbz` — not a synthetic fixture. This file is
//! intentionally not present in version control (see `tests/fixtures/README.md`
//! and `.gitignore`'s `*.cbz` rule), so this test skips itself gracefully
//! when it isn't there rather than failing CI or another contributor's
//! machine.
//!
//! To run it locally: drop any Mangabind-produced `.cbz` at the mangapress
//! workspace root and re-run `cargo test`.

use mangapress_core::archive::cbz::extract_cbz;
use mangapress_core::ebook::epub::{build_epub, EpubOptions};
use mangapress_core::ebook::group_into_chapters;
use mangapress_core::manga::ReadingDirection;
use std::path::PathBuf;

fn real_cbz_path() -> Option<PathBuf> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .ok()?;
    std::fs::read_dir(&workspace_root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("cbz"))
}

#[test]
fn builds_a_valid_epub_from_a_real_mangabind_cbz() {
    let Some(path) = real_cbz_path() else {
        eprintln!("no .cbz found at the workspace root — skipping (see module docs)");
        return;
    };
    eprintln!("using real fixture: {}", path.display());

    let entries = extract_cbz(&path).expect("extracting the real cbz should succeed");
    assert!(!entries.is_empty(), "the real cbz should contain files");

    let chapters = group_into_chapters(entries);
    println!("detected {} chapter(s):", chapters.len());
    for chapter in &chapters {
        println!("  {:?} -> {} page(s)", chapter.title, chapter.pages.len());
    }
    assert!(
        chapters.len() > 1,
        "a real multi-chapter volume should produce more than one chapter"
    );
    for chapter in &chapters {
        assert!(
            !chapter.pages.is_empty(),
            "chapter {:?} should not be empty",
            chapter.title
        );
    }

    let options = EpubOptions {
        title: path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string()),
        author: "Unknown".to_string(),
        language: "en".to_string(),
        reading_direction: ReadingDirection {
            right_to_left: true,
        },
        description: None,
    };

    let epub_bytes = build_epub(&chapters, &options).expect("building the EPUB should succeed");
    assert!(
        epub_bytes.len() > 1024,
        "the EPUB should not be trivially small"
    );

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&epub_bytes))
        .expect("output must be a valid zip");
    assert_eq!(archive.by_index(0).unwrap().name(), "mimetype");

    let mut ncx = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("OEBPS/toc.ncx").unwrap(), &mut ncx)
        .unwrap();
    for chapter in &chapters {
        let escaped = chapter.title.replace('\'', "&apos;");
        assert!(
            ncx.contains(&escaped),
            "toc.ncx should list chapter {:?}",
            chapter.title
        );
    }
    assert_eq!(ncx.matches("<navPoint").count(), chapters.len());

    let out_path = std::env::temp_dir().join("mangapress-real-fixture-test.epub");
    std::fs::write(&out_path, &epub_bytes).expect("writing the built EPUB for manual inspection");
    println!("wrote {} bytes to {}", epub_bytes.len(), out_path.display());
}
