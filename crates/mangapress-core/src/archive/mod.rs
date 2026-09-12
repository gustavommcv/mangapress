//! Archive input/output. v1 scope is `.cbz` (a plain ZIP) and bare folders
//! of images only — matching the actual Mangabind->mangapress contract
//! (see `docs/adr/0005-mangabind-contract.md`). CBR/7z/RAR input is
//! explicitly out of scope for now: KCC itself hard-requires the external
//! `7z` binary for those with no pure-Python fallback, and the `zip` crate
//! gives us a pure-Rust path for the one format that actually matters here.

pub mod cbz;

/// One extracted source page, in tree order, with its path relative to the
/// archive/folder root — the relative path is what chapter attribution
/// keys off of (see [`crate::ebook::epub`] and the ADR on the Mangabind
/// contract's basename-collision fix).
pub struct SourceEntry {
    pub relative_path: std::path::PathBuf,
    pub bytes: Vec<u8>,
}
