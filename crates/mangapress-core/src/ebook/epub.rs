//! EPUB generation: hand-built OPF/NCX/NAV/XHTML, same approach upstream
//! KCC takes (there's no off-the-shelf crate that produces KCC-equivalent
//! fixed-layout, per-page-spread-property, RTL-aware EPUBs).
//!
//! Chapter/TOC contract with Mangabind (`docs/adr/0005-mangabind-contract.md`):
//! each [`super::Chapter`] becomes one `navPoint`/`<li>` in `toc.ncx` and
//! `nav.xhtml`, labeled with the *original* directory name (not the
//! slugified/renamed one used for internal file paths) — reproducing what a
//! Mangabind-produced `.cbz` already gets from upstream KCC today, but
//! keying chapter titles by full relative path so two identically-named
//! subfolders in different branches don't clobber each other (upstream's
//! `chapterNames` dict is keyed by basename only — see the KCC research
//! notes referenced in that ADR).

use super::Chapter;
use crate::error::Result;
use crate::manga::ReadingDirection;

pub struct EpubOptions {
    pub title: String,
    pub author: String,
    pub language: String,
    pub reading_direction: ReadingDirection,
}

pub fn build_epub(_chapters: &[Chapter], _options: &EpubOptions) -> Result<Vec<u8>> {
    todo!("build OPF/NCX/NAV/XHTML, zip via archive::cbz::write_zip with mimetype first and stored")
}
