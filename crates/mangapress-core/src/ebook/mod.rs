//! Output format builders. No PDF-rendering library dependency is needed
//! anywhere here — mangapress's scope is images/`.cbz` *in*, never PDF in,
//! so [`pdf`] only ever composes already-processed raster pages onto PDF
//! pages (`printpdf`), unlike upstream KCC which also uses PyMuPDF to
//! *read* PDF input.
//!
//! MOBI/AZW3 is deliberately out of scope for now — see
//! `docs/adr/0006-mobi-azw3-deferred.md`.

pub mod cbz_out;
pub mod epub;
pub mod pdf;

/// One chapter's worth of already-processed pages, keyed by its *full
/// relative path* from the archive root (not just its basename — see
/// `docs/adr/0005-mangabind-contract.md` for why upstream KCC's
/// basename-only keying is a bug we're deliberately not inheriting).
pub struct Chapter {
    pub relative_path: std::path::PathBuf,
    pub title: String,
    pub pages: Vec<Vec<u8>>,
}
