//! `ComicInfo.xml` reading (title/author/series/volume, per-page bookmarks).
//!
//! Port target: `MetadataParser` in KCC's `metadata.py`. Lower priority than
//! the image pipeline and EPUB TOC generation — the Mangabind contract
//! doesn't currently depend on `ComicInfo.xml` bookmarks (it derives
//! chapters from folder structure), so this can lag behind.

#[derive(Debug, Clone, Default)]
pub struct ComicInfo {
    pub series: Option<String>,
    pub volume: Option<u32>,
    pub number: Option<String>,
    pub title: Option<String>,
    pub writer: Option<String>,
    pub summary: Option<String>,
}

pub fn parse_comic_info_xml(_xml: &str) -> crate::error::Result<ComicInfo> {
    todo!("parse ComicInfo.xml — not on the critical path for the Mangabind contract, implement after EPUB output works")
}
