//! `.cbz` (ZIP) reading and writing, via the `zip` crate — no external `7z`
//! binary required, unlike upstream KCC's `comicarchive.py`.

use super::SourceEntry;
use crate::error::Result;
use std::path::Path;

/// Extract every image file from a `.cbz`, preserving relative paths (this
/// is what makes the Mangabind chapter-subfolder contract work — see
/// `docs/adr/0005-mangabind-contract.md`). Entries are returned in the
/// order the crate's own `walkSort`-equivalent natural sort produces, not
/// raw ZIP central-directory order.
pub fn extract_cbz(_path: &Path) -> Result<Vec<SourceEntry>> {
    todo!("open with zip::ZipArchive, natural-sort entries by relative path, read each into memory")
}

/// Package processed output files into a `.cbz`/`.epub` container.
/// `store_uncompressed` mirrors KCC's own choice for its ZIP fallback path
/// (images are already compressed, so re-compressing wastes CPU for no
/// size benefit) — EPUB's `mimetype` entry additionally needs to be first
/// and stored, per the EPUB OCF spec.
pub fn write_zip(_entries: &[(String, Vec<u8>)], _store_uncompressed: bool) -> Result<Vec<u8>> {
    todo!("build with zip::ZipWriter")
}

#[allow(dead_code)]
fn read_all(mut r: impl std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
}
