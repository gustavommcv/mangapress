//! `.cbz` (ZIP) reading and writing, via the `zip` crate — no external `7z`
//! binary required, unlike upstream KCC's `comicarchive.py` (which
//! hard-requires it for extraction, even of a plain ZIP — see
//! `docs/adr/0002-pip-workaround-tested-and-rejected.md`).

use super::SourceEntry;
use crate::error::Result;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

/// Extract every file from a `.cbz`, preserving relative paths (this is
/// what makes the Mangabind chapter-subfolder contract work — see
/// `docs/adr/0005-mangabind-contract.md`). Directory entries are skipped.
/// Entries are returned in [`crate::natural_sort`] order, not raw ZIP
/// central-directory order.
pub fn extract_cbz(path: &Path) -> Result<Vec<SourceEntry>> {
    extract_from_reader(std::fs::File::open(path)?)
}

/// Same as [`extract_cbz`], from an already-open reader — the entry point
/// `extract_cbz` itself is just a thin file-opening wrapper around this, so
/// tests (and any future non-filesystem source) can exercise the real ZIP
/// parsing logic without touching disk.
pub fn extract_from_reader<R: Read + Seek>(reader: R) -> Result<Vec<SourceEntry>> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut entries = Vec::with_capacity(archive.len());

    for i in 0..archive.len() {
        let mut zip_entry = archive.by_index(i)?;
        if zip_entry.is_dir() {
            continue;
        }
        let relative_path = PathBuf::from(zip_entry.name());
        let mut bytes = Vec::with_capacity(zip_entry.size() as usize);
        zip_entry.read_to_end(&mut bytes)?;
        entries.push(SourceEntry {
            relative_path,
            bytes,
        });
    }

    entries.sort_by(|a, b| crate::natural_sort::compare_paths(&a.relative_path, &b.relative_path));
    Ok(entries)
}

/// Package named byte blobs into a ZIP archive. `store_uncompressed`
/// mirrors KCC's own choice for its ZIP fallback path (images are already
/// compressed, so re-compressing wastes CPU for no size benefit) — EPUB's
/// `mimetype` entry additionally needs to be first and stored, per the EPUB
/// OCF spec, which callers get by simply listing it first in `entries`.
///
/// `entries` names must already be `/`-separated (use
/// [`path_to_entry_name`] to convert a [`Path`] correctly on every
/// platform, since ZIP entry names are never OS paths).
pub fn write_zip(entries: &[(String, Vec<u8>)], store_uncompressed: bool) -> Result<Vec<u8>> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let method = if store_uncompressed {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let options = zip::write::SimpleFileOptions::default().compression_method(method);

        for (name, bytes) in entries {
            writer.start_file(name, options)?;
            writer.write_all(bytes)?;
        }
        writer.finish()?;
    }
    Ok(buffer.into_inner())
}

/// Convert a [`Path`] into a `/`-separated ZIP entry name, regardless of
/// the host OS's own path separator.
pub fn path_to_entry_name(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample_cbz() -> Vec<u8> {
        write_zip(
            &[
                (
                    "c002 - Chapter Two/page001.jpg".to_string(),
                    b"ch2-p1".to_vec(),
                ),
                (
                    "c001 - Chapter One/page002.jpg".to_string(),
                    b"ch1-p2".to_vec(),
                ),
                (
                    "c001 - Chapter One/page010.jpg".to_string(),
                    b"ch1-p10".to_vec(),
                ),
                (
                    "c001 - Chapter One/page001.jpg".to_string(),
                    b"ch1-p1".to_vec(),
                ),
            ],
            false,
        )
        .expect("building the sample cbz should succeed")
    }

    #[test]
    fn extracts_every_file_with_correct_bytes() {
        let entries = extract_from_reader(Cursor::new(sample_cbz())).unwrap();
        assert_eq!(entries.len(), 4);
        let by_name: std::collections::HashMap<_, _> = entries
            .iter()
            .map(|e| {
                (
                    e.relative_path.to_string_lossy().replace('\\', "/"),
                    &e.bytes,
                )
            })
            .collect();
        assert_eq!(
            by_name["c001 - Chapter One/page001.jpg"].as_slice(),
            b"ch1-p1"
        );
        assert_eq!(
            by_name["c002 - Chapter Two/page001.jpg"].as_slice(),
            b"ch2-p1"
        );
    }

    #[test]
    fn extracted_entries_are_naturally_sorted() {
        let entries = extract_from_reader(Cursor::new(sample_cbz())).unwrap();
        let names: Vec<String> = entries
            .iter()
            .map(|e| e.relative_path.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            vec![
                "c001 - Chapter One/page001.jpg",
                "c001 - Chapter One/page002.jpg",
                "c001 - Chapter One/page010.jpg",
                "c002 - Chapter Two/page001.jpg",
            ]
        );
    }

    #[test]
    fn directory_entries_are_skipped() {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            writer
                .add_directory(
                    "c001 - Chapter One/",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer
                .start_file(
                    "c001 - Chapter One/page001.jpg",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(b"hello").unwrap();
            writer.finish().unwrap();
        }
        let entries = extract_from_reader(Cursor::new(buffer.into_inner())).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bytes, b"hello");
    }

    #[test]
    fn write_then_extract_round_trips_uncompressed() {
        let bytes = write_zip(&[("only.txt".to_string(), b"payload".to_vec())], true).unwrap();
        let entries = extract_from_reader(Cursor::new(bytes)).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].bytes, b"payload");
    }

    #[test]
    fn path_to_entry_name_uses_forward_slashes() {
        let path = PathBuf::from("c001 - Title").join("page001.jpg");
        assert_eq!(path_to_entry_name(&path), "c001 - Title/page001.jpg");
    }
}
