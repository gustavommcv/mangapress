use super::{xml_escape, Chapter, EpubOptions};

pub(super) fn build_ncx(
    options: &EpubOptions,
    identifier: &str,
    chapters: &[&Chapter],
    chapter_hrefs: &[String],
    nested: bool,
) -> String {
    let mut id = 0usize;
    // NCX entries sharing a content target must share playOrder, even when
    // their IDs and labels differ (a volume and its first chapter, or aliases).
    let mut targets = std::collections::HashMap::new();
    let mut play_order = |href: &str| {
        let next = targets.len() + 1;
        *targets.entry(href.to_owned()).or_insert(next)
    };
    let nav_points: String = if nested {
        group_by_volume(chapters, chapter_hrefs)
            .into_iter()
            .map(|volume| {
                id += 1;
                let volume_id = id;
                let volume_order = play_order(volume.chapters[0].1);
                let children: String = volume
                    .chapters
                    .iter()
                    .map(|(chapter, href)| {
                        id += 1;
                        let chapter_order = play_order(href);
                        format!(
                            r#"<navPoint id="chapter{id}" playOrder="{chapter_order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/></navPoint>"#,
                            title = xml_escape(toc_label(chapter, &options.title)),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    r#"<navPoint id="volume{volume_id}" playOrder="{volume_order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/>
{children}
</navPoint>"#,
                    title = xml_escape(&volume.title),
                    href = volume.chapters[0].1,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        chapters
            .iter()
            .zip(chapter_hrefs)
            .map(|(chapter, href)| {
                id += 1;
                let chapter_order = play_order(href);
                format!(
                    r#"<navPoint id="chapter{id}" playOrder="{chapter_order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/></navPoint>"#,
                    title = xml_escape(toc_label(chapter, &options.title)),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx version="2005-1" xml:lang="{lang}" xmlns="http://www.daisy.org/z3986/2005/ncx/">
<head>
<meta name="dtb:uid" content="{identifier}"/>
<meta name="dtb:depth" content="{depth}"/>
<meta name="dtb:totalPageCount" content="0"/>
<meta name="dtb:maxPageNumber" content="0"/>
<meta name="generated" content="true"/>
</head>
<docTitle><text>{title}</text></docTitle>
<navMap>
{nav_points}
</navMap>
</ncx>"#,
        lang = xml_escape(&options.language),
        identifier = xml_escape(identifier),
        title = xml_escape(&options.title),
        depth = if nested { 2 } else { 1 },
    )
}

pub(super) fn build_nav(
    options: &EpubOptions,
    chapters: &[&Chapter],
    chapter_hrefs: &[String],
    nested: bool,
) -> String {
    let items: String = if nested {
        group_by_volume(chapters, chapter_hrefs)
            .into_iter()
            .map(|volume| {
                let children: String = volume
                    .chapters
                    .iter()
                    .map(|(chapter, href)| {
                        format!(
                            r#"<li><a href="{href}">{title}</a></li>"#,
                            title = xml_escape(toc_label(chapter, &options.title)),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    r#"<li><a href="{href}">{title}</a><ol>
{children}
</ol></li>"#,
                    title = xml_escape(&volume.title),
                    href = volume.chapters[0].1,
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        chapters
            .iter()
            .zip(chapter_hrefs)
            .map(|(chapter, href)| {
                format!(
                    r#"<li><a href="{href}">{title}</a></li>"#,
                    title = xml_escape(toc_label(chapter, &options.title)),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
<title>{title}</title>
<meta charset="utf-8"/>
</head>
<body>
<nav epub:type="toc" id="toc">
<ol>
{items}
</ol>
</nav>
<nav epub:type="page-list">
<ol>
{page_list}
</ol>
</nav>
</body>
</html>"#,
        title = xml_escape(&options.title),
        // Upstream's page list is the chapter list again, flat: one entry
        // per chapter, pointing at its first page.
        page_list = chapters
            .iter()
            .zip(chapter_hrefs)
            .map(|(chapter, href)| {
                format!(
                    r#"<li><a href="{href}">{title}</a></li>"#,
                    title = xml_escape(toc_label(chapter, &options.title)),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// What the table of contents calls a chapter. Pages lying directly in the book have no
/// folder to name them, so the book's own title stands in, as upstream does; a bookmark has
/// no pages and keeps the name it was given.
fn toc_label<'a>(chapter: &'a Chapter, book_title: &'a str) -> &'a str {
    if chapter.relative_path.as_os_str().is_empty() && !chapter.pages.is_empty() {
        book_title
    } else {
        &chapter.title
    }
}

/// One volume's worth of already-flat chapters, grouped for `nested_toc`
/// output only. Built purely from each [`Chapter`]'s existing
/// `relative_path` - its parent-of-parent directory is the volume, matching
/// how Mangabind's `-combine` mode nests `<volume dir>/<chapter dir>/...`
/// (see `docs/adr/0005-mangabind-contract.md` for why chapters are already
/// grouped by full path, and `docs/adr/0012-nested-toc-for-combined-volumes.md`
/// for this one more level). No change to [`crate::ebook::group_into_chapters`] or
/// the [`Chapter`] type itself was needed for this.
struct VolumeGroup<'a> {
    title: String,
    chapters: Vec<(&'a Chapter, &'a String)>,
}

fn group_by_volume<'a>(
    chapters: &[&'a Chapter],
    chapter_hrefs: &'a [String],
) -> Vec<VolumeGroup<'a>> {
    let mut order: Vec<String> = Vec::new();
    let mut by_volume: std::collections::HashMap<String, Vec<(&Chapter, &String)>> =
        std::collections::HashMap::new();

    for (chapter, href) in chapters.iter().zip(chapter_hrefs) {
        let volume_title = chapter
            .relative_path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string());
        if !by_volume.contains_key(&volume_title) {
            order.push(volume_title.clone());
        }
        by_volume
            .entry(volume_title)
            .or_default()
            .push((*chapter, href));
    }

    order
        .into_iter()
        .map(|title| {
            let chapters = by_volume.remove(&title).expect("just inserted above");
            VolumeGroup { title, chapters }
        })
        .collect()
}

/// The table-of-contents entries `ComicInfo.xml`'s bookmarks ask for: one
/// per bookmark that names a page this book actually has, as a title-only
/// [`Chapter`] plus the href of that page.
///
/// A bookmark names a *source* page; its entry points at the first output
/// page that source page became, found by counting pages rather than by
/// arithmetic. Upstream instead adds a fixed amount per split spread it
/// finds while scanning a window of the output — a window sized before the
/// scan, so a run of spreads just before a bookmark is undercounted and the
/// entry lands a page early (three split spreads immediately before a
/// bookmark put it on the last one's second half). Where the two differ,
/// this is the page the bookmark names. A bookmark past the last page is
/// skipped; upstream stops with an error there.
pub(super) fn bookmark_toc(
    bookmarks: &[(u32, String)],
    source_page_starts: &[usize],
    page_hrefs: &[String],
) -> Vec<(Chapter, String)> {
    bookmarks
        .iter()
        .filter_map(|(source_page, title)| {
            let output_page = *source_page_starts.get(*source_page as usize)?;
            Some((
                Chapter {
                    relative_path: std::path::PathBuf::new(),
                    title: title.clone(),
                    pages: Vec::new(),
                },
                page_hrefs[output_page].clone(),
            ))
        })
        .collect()
}
