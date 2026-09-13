//! `ComicInfo.xml` reading and title/author resolution.
//!
//! Port target: `MetadataParser` and `getMetadata()`'s title/author
//! precedence logic in KCC's `metadata.py`/`comic2ebook.py`. Unlike most of
//! `mangapress-core`'s other port targets, `metadata.py` carries a normal
//! ISC header upstream (see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md`
//! for which files *don't*) — still reimplemented in Rust idiom here
//! rather than transliterated, but with less of the extra GPL-boundary
//! caution that file needs.
//!
//! Scope note: `ComicInfo.xml`'s per-page `Bookmarks` (`<Page Image="N"
//! Bookmark="..."/>`) can override upstream's folder-derived chapter list
//! entirely — this needs global-page-index bookkeeping that gets adjusted
//! for every page a spread-split turns into two (`comic2ebook.py`'s
//! `buildEPUB()`, the `-kcc-b`-suffix-counting loop). This project's
//! `ebook::group_into_chapters` derives chapters from folder structure
//! per-chapter, not from one global flat page list, and the Mangabind
//! contract (`docs/adr/0005-mangabind-contract.md`) doesn't depend on
//! bookmarks at all — so `ComicInfo::bookmarks` is parsed and available,
//! but *not yet wired into chapter overriding*. Everything else
//! (title/series/volume/number/authors/summary resolution, and
//! `--keepcomicinfo` preservation into CBZ output) is fully implemented.

use std::path::Path;

/// Parsed `ComicInfo.xml` contents. Fields are `None`/empty when the
/// corresponding element is absent, matching upstream's "only overwrite if
/// present" behavior rather than upstream's own empty-string defaults.
#[derive(Debug, Clone, Default)]
pub struct ComicInfo {
    pub series: Option<String>,
    /// Kept as a string, not parsed to a number: upstream only ever
    /// zero-pads it as text (`Volume.zfill(2)`) and never does arithmetic
    /// on it.
    pub volume: Option<String>,
    pub number: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub writers: Vec<String>,
    pub pencillers: Vec<String>,
    pub inkers: Vec<String>,
    pub colorists: Vec<String>,
    /// `(page_index, bookmark_name)`, parsed but not yet consumed — see
    /// module docs.
    pub bookmarks: Vec<(u32, String)>,
}

pub fn parse_comic_info_xml(xml: &str) -> crate::error::Result<ComicInfo> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| crate::error::Error::ComicInfo(e.to_string()))?;

    let text_of = |tag: &str| -> Option<String> {
        doc.descendants()
            .find(|n| n.is_element() && n.has_tag_name(tag))
            .and_then(|n| n.text())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let people_of = |tag: &str| -> Vec<String> {
        let mut people: Vec<String> = text_of(tag)
            .map(|s| {
                s.split(", ")
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        people.sort();
        people.dedup();
        people
    };

    let bookmarks: Vec<(u32, String)> = doc
        .descendants()
        .filter(|n| n.is_element() && n.has_tag_name("Page"))
        .filter_map(|n| {
            let image = n.attribute("Image")?.parse::<u32>().ok()?;
            let bookmark = n.attribute("Bookmark")?;
            if bookmark.is_empty() {
                None
            } else {
                Some((image, bookmark.to_string()))
            }
        })
        .collect();

    Ok(ComicInfo {
        series: text_of("Series"),
        volume: text_of("Volume"),
        number: text_of("Number"),
        title: text_of("Title"),
        summary: text_of("Summary"),
        writers: people_of("Writer"),
        pencillers: people_of("Penciller"),
        inkers: people_of("Inker"),
        colorists: people_of("Colorist"),
        bookmarks,
    })
}

/// `--metadatatitle`'s three modes, matching upstream's `0`/`1`/`2` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataTitleMode {
    /// Use `Series`/`Volume`/`Number` only, never `ComicInfo.xml`'s own
    /// `Title` field.
    #[default]
    SeriesOnly,
    /// Append `: Title` after `Series`/`Volume`/`Number`.
    Combine,
    /// Use `ComicInfo.xml`'s `Title` field alone — overrides even a title
    /// the user explicitly passed with `-t`, matching upstream exactly
    /// (confirmed by reading `getMetadata()`: this check comes *before*
    /// the "did the user set a title" branch, not after).
    TitleOnly,
}

#[derive(Debug, Clone)]
pub struct ResolvedMetadata {
    pub title: String,
    pub authors: Vec<String>,
    pub summary: Option<String>,
}

/// `getMetadata()`'s title/author precedence. `fallback_title` is what
/// upstream derives from the input path's basename when the user passes
/// neither `-t` nor a title-bearing `ComicInfo.xml`.
///
/// One deliberate divergence: upstream's ultimate fallback author is the
/// literal string `"KCC"` (self-attributing the *converter* as author when
/// nothing else is known). Self-attributing mangapress the same way makes
/// even less sense for a tool with no relation to the work's actual
/// authorship, so the fallback here is `"Unknown"` instead.
pub fn resolve(
    info: Option<&ComicInfo>,
    user_title: Option<&str>,
    user_author: Option<&str>,
    fallback_title: &str,
    metadata_title_mode: MetadataTitleMode,
) -> ResolvedMetadata {
    let default_title = user_title.is_none();
    let default_author = user_author.is_none();

    let mut title = user_title
        .map(str::to_string)
        .unwrap_or_else(|| fallback_title.to_string());
    let mut authors: Vec<String> = user_author.map(|a| vec![a.to_string()]).unwrap_or_default();
    let mut summary = None;

    if let Some(info) = info {
        summary = info.summary.clone();

        if metadata_title_mode == MetadataTitleMode::TitleOnly {
            if let Some(t) = &info.title {
                title = t.clone();
            }
        } else if default_title {
            if let Some(series) = &info.series {
                title = series.clone();
            }
            let mut suffix = String::new();
            if let Some(volume) = &info.volume {
                suffix.push_str(&format!(" Vol. {volume:0>2}"));
            }
            if let Some(number) = &info.number {
                suffix.push_str(&format!(" #{number:0>3}"));
            }
            if metadata_title_mode == MetadataTitleMode::Combine {
                if let Some(t) = &info.title {
                    suffix.push_str(&format!(": {t}"));
                }
            }
            title.push_str(&suffix);
        }

        if default_author {
            let mut people: Vec<String> = info
                .writers
                .iter()
                .chain(info.pencillers.iter())
                .chain(info.inkers.iter())
                .chain(info.colorists.iter())
                .cloned()
                .collect();
            people.sort();
            people.dedup();
            authors = people;
        }
    }

    if authors.is_empty() {
        authors = vec!["Unknown".to_string()];
    }

    ResolvedMetadata {
        title,
        authors,
        summary,
    }
}

/// Extracts a top-level `ComicInfo.xml` entry from already-extracted
/// archive entries, removing it from the list (matching upstream's
/// `os.remove(xmlPath)` before the chapter-building walk, so it's never
/// mistaken for a page image) and returning its raw bytes for both parsing
/// and, if `--keepcomicinfo` is set, re-embedding into CBZ output.
///
/// Only matches at the archive root — a `ComicInfo.xml` inside a chapter
/// subfolder is left alone (and would be filtered out downstream anyway,
/// same as any other non-image file, once `group_into_chapters`/page
/// processing gets to it).
pub fn extract_comic_info_entry(entries: &mut Vec<crate::archive::SourceEntry>) -> Option<Vec<u8>> {
    let root_xml = Path::new("ComicInfo.xml");
    let position = entries
        .iter()
        .position(|entry| entry.relative_path == root_xml)?;
    Some(entries.remove(position).bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ComicInfo xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Series>Chainsaw Man</Series>
  <Volume>1</Volume>
  <Number>001</Number>
  <Title>A Dog and a Chainsaw</Title>
  <Summary>Denji fights devils.</Summary>
  <Writer>Tatsuki Fujimoto</Writer>
  <Penciller>Tatsuki Fujimoto, Someone Else</Penciller>
  <Pages>
    <Page Image="0" />
    <Page Image="5" Bookmark="Chapter 2" />
    <Page Image="12" Bookmark="Chapter 3" />
  </Pages>
</ComicInfo>"#;

    #[test]
    fn parses_scalar_fields() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        assert_eq!(info.series.as_deref(), Some("Chainsaw Man"));
        assert_eq!(info.volume.as_deref(), Some("1"));
        assert_eq!(info.number.as_deref(), Some("001"));
        assert_eq!(info.title.as_deref(), Some("A Dog and a Chainsaw"));
        assert_eq!(info.summary.as_deref(), Some("Denji fights devils."));
    }

    #[test]
    fn dedupes_and_sorts_people_lists() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        assert_eq!(info.writers, vec!["Tatsuki Fujimoto".to_string()]);
        assert_eq!(
            info.pencillers,
            vec!["Someone Else".to_string(), "Tatsuki Fujimoto".to_string()]
        );
    }

    #[test]
    fn missing_fields_are_none_not_empty_string() {
        let info = parse_comic_info_xml("<ComicInfo></ComicInfo>").unwrap();
        assert_eq!(info.series, None);
        assert!(info.writers.is_empty());
    }

    #[test]
    fn parses_only_pages_with_both_image_and_bookmark() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        assert_eq!(
            info.bookmarks,
            vec![(5, "Chapter 2".to_string()), (12, "Chapter 3".to_string())]
        );
    }

    #[test]
    fn resolve_falls_back_to_provided_title_and_unknown_author_with_no_comicinfo() {
        let resolved = resolve(
            None,
            None,
            None,
            "My Folder Name",
            MetadataTitleMode::SeriesOnly,
        );
        assert_eq!(resolved.title, "My Folder Name");
        assert_eq!(resolved.authors, vec!["Unknown".to_string()]);
    }

    #[test]
    fn resolve_series_only_mode_builds_series_plus_volume_and_number() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            None,
            None,
            "fallback",
            MetadataTitleMode::SeriesOnly,
        );
        assert_eq!(resolved.title, "Chainsaw Man Vol. 01 #001");
    }

    #[test]
    fn resolve_combine_mode_appends_the_xml_title() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            None,
            None,
            "fallback",
            MetadataTitleMode::Combine,
        );
        assert_eq!(
            resolved.title,
            "Chainsaw Man Vol. 01 #001: A Dog and a Chainsaw"
        );
    }

    #[test]
    fn resolve_title_only_mode_overrides_even_an_explicit_user_title() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            Some("User Provided Title"),
            None,
            "fallback",
            MetadataTitleMode::TitleOnly,
        );
        assert_eq!(resolved.title, "A Dog and a Chainsaw");
    }

    #[test]
    fn resolve_never_touches_an_explicit_user_title_outside_title_only_mode() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            Some("User Provided Title"),
            None,
            "fallback",
            MetadataTitleMode::Combine,
        );
        assert_eq!(resolved.title, "User Provided Title");
    }

    #[test]
    fn resolve_never_touches_an_explicit_user_author() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            None,
            Some("Explicit Author"),
            "fallback",
            MetadataTitleMode::SeriesOnly,
        );
        assert_eq!(resolved.authors, vec!["Explicit Author".to_string()]);
    }

    #[test]
    fn resolve_builds_author_list_from_all_role_fields_when_not_overridden() {
        let info = parse_comic_info_xml(SAMPLE_XML).unwrap();
        let resolved = resolve(
            Some(&info),
            None,
            None,
            "fallback",
            MetadataTitleMode::SeriesOnly,
        );
        assert_eq!(
            resolved.authors,
            vec!["Someone Else".to_string(), "Tatsuki Fujimoto".to_string()]
        );
    }

    #[test]
    fn extract_comic_info_entry_removes_only_the_root_level_file() {
        use crate::archive::SourceEntry;
        use std::path::PathBuf;

        let mut entries = vec![
            SourceEntry {
                relative_path: PathBuf::from("ComicInfo.xml"),
                bytes: b"root".to_vec(),
            },
            SourceEntry {
                relative_path: PathBuf::from("c001/ComicInfo.xml"),
                bytes: b"nested".to_vec(),
            },
            SourceEntry {
                relative_path: PathBuf::from("c001/p0001.jpg"),
                bytes: b"page".to_vec(),
            },
        ];

        let extracted = extract_comic_info_entry(&mut entries);
        assert_eq!(extracted, Some(b"root".to_vec()));
        assert_eq!(entries.len(), 2);
        assert!(entries
            .iter()
            .any(|e| e.relative_path == std::path::Path::new("c001/ComicInfo.xml")));
    }

    #[test]
    fn extract_comic_info_entry_returns_none_when_absent() {
        let mut entries: Vec<crate::archive::SourceEntry> = Vec::new();
        assert_eq!(extract_comic_info_entry(&mut entries), None);
    }
}
