//! EPUB generation: hand-built OCF/OPF/NCX/NAV/XHTML, the same approach
//! upstream KCC takes (there's no off-the-shelf crate that produces
//! KCC-equivalent fixed-layout, RTL-aware, chapter-per-folder EPUBs).
//!
//! Chapter/TOC contract with Mangabind (`docs/adr/0005-mangabind-contract.md`):
//! each [`super::Chapter`] becomes one `navPoint`/`<li>` in `toc.ncx` and
//! `nav.xhtml`, labeled with the *original* directory name — reproducing
//! what a Mangabind-produced `.cbz` already gets from upstream KCC today.
//!
//! Internal file paths (`Text/cNNNN/pNNNN.xhtml`, `Images/cNNNN/pNNNN.ext`)
//! are purely sequential, deliberately decoupled from the chapter's display
//! title — upstream slugifies directory names for this instead, which is a
//! whole extra step (and failure mode, for exotic characters) this project
//! doesn't need: the title only ever needs to be human-readable text in the
//! TOC, never a filesystem-safe path segment.
//!
//! The package itself follows upstream's `buildOPF()`/`buildNCX()`/
//! `buildNAV()`/`buildHTML()` (KCC 12.0.0): one `<dc:creator>` per author, a
//! generator `<dc:contributor>`, `dcterms:modified`, the Kindle fixed-layout
//! `<meta>` block for a Kindle profile, `rendition:spread`, a
//! `page-spread-*` property on every spine item, a `page-list` nav beside
//! the table of contents, and a black page background behind a dark page.
//! The cover is upstream's too: a separate `Images/cover.jpg` made from the
//! book's untouched first image (see [`super::cover`]), not the processed
//! first page. `ComicInfo.xml` page bookmarks replace the folder-derived
//! table of contents when the book has any, as they do upstream (see
//! [`EpubOptions::bookmarks`]).
//!
//! Where this file still differs from upstream it is on purpose, and says so
//! where it does: the identifier is derived from the book rather than drawn
//! at random (see [`synthetic_identifier`]); `toc.ncx` keeps the `playOrder`
//! the NCX schema requires, which upstream leaves out; a bookmark is placed
//! on the exact page it names, where upstream's own arithmetic can land one
//! page early (see [`bookmark_toc`]); and the two stylesheet rules are
//! written into each page instead of a shared `style.css`. Not carried over:
//! everything that only exists for Kindle's Panel View.
//! Centered spine items always use the reserved `rendition:` property;
//! KCC's bare Kindle spelling is undefined (ADR 0018).

use super::Chapter;
use crate::error::{Error, Result};
use crate::manga::ReadingDirection;
use crate::pipeline::spread::PageRole;

pub struct EpubOptions {
    pub title: String,
    /// One `<dc:creator>` element each, as upstream writes them.
    pub authors: Vec<String>,
    pub language: String,
    pub reading_direction: ReadingDirection,
    /// `ComicInfo.xml`'s `Summary`, if any (see [`crate::metadata`]).
    pub description: Option<String>,
    /// Expect each [`Chapter`]'s `relative_path` to carry one more directory
    /// level above it (a volume) and build a two-level `toc.ncx`/`nav.xhtml`
    /// (a volume entry as parent, its chapters nested underneath) instead of
    /// today's flat, one-entry-per-chapter list. Mangabind's `-combine` mode
    /// is the one producer of this shape today; see
    /// `docs/adr/0012-nested-toc-for-combined-volumes.md`. False leaves
    /// every existing input and output byte-for-byte unchanged.
    pub nested_toc: bool,
    /// The device profile is a Kindle one (upstream's `iskindle`): every page
    /// carries upstream's hidden first block, and left/right spine items use
    /// the older `page-spread-*` spelling. Center uses `rendition:` on every family.
    pub kindle: bool,
    /// The effective EPUB target for an unmodified Kindle profile,
    /// including Scribe's width cap — writes the Kindle fixed-layout `<meta>` block,
    /// whose `original-resolution` this is. `None` for any other profile, and
    /// for a Kindle profile overridden by `--customwidth`/`--customheight`
    /// (upstream's "Custom" profile, which gets no such block).
    pub kindle_resolution: Option<(u32, u32)>,
    /// `--invertdirection`: turn pages the opposite way to the reading
    /// order — the spine's page progression, the Kindle writing mode and
    /// the side the first page sits on all flip. Which half of a split
    /// spread comes first does not.
    pub invert_direction: bool,
    /// `--spreadshift`: start the book on the opposite side of a two-page
    /// view, to line spreads up.
    pub spread_shift: bool,
    /// `--onepagelandscape`: every page centered alone in a two-page view.
    pub one_page_landscape: bool,
    /// The cover, already encoded as JPEG — see [`super::cover::build_cover`].
    /// Written as `Images/cover.jpg` and declared as the publication's cover
    /// image, apart from the pages. `None` declares the first page's own
    /// image as the cover instead, which is all a caller with no source
    /// image to build one from can do.
    pub cover: Option<Vec<u8>>,
    /// `ComicInfo.xml`'s page bookmarks: `(source page index, title)`, the
    /// index counting the book's source images from zero. When there are
    /// any, the table of contents lists these instead of the folders — one
    /// entry per bookmark, in this order, each pointing at the page it
    /// names — exactly as upstream replaces its chapter list. Pages keep
    /// their folder-derived paths either way.
    pub bookmarks: Vec<(u32, String)>,
    /// The series this book belongs to and, if known, its position in it
    /// (see [`crate::metadata::ResolvedMetadata`]). Written as EPUB 3
    /// collection metadata for every profile that isn't a Kindle one, as
    /// upstream does — it is what puts a volume under its series on a Kobo.
    pub series: Option<(String, Option<String>)>,
    /// `dcterms:modified`, as `YYYY-MM-DDThh:mm:ssZ`. EPUB 3 requires it;
    /// passed in rather than read from the clock here so that building the
    /// same book twice still gives the same bytes when the caller wants that.
    pub modified: String,
}

/// Which side of a two-page view each page belongs on — upstream's
/// `page-spread-*` assignment, for readers that show two pages at once.
///
/// Going forward, ordinary pages alternate, starting on the side reading
/// begins from (right for right-to-left). A split spread's halves take that
/// side and the opposite one, a rotated spread sits in the center, and after
/// either the alternation starts over. Then, going backward from the last
/// spread in the book, the ordinary pages *before* each spread are
/// re-assigned so that they alternate up to it and the spread's halves
/// always land on a fresh pair — without that second pass a spread preceded
/// by an odd number of pages would straddle two page-turns.
///
/// `start_on_second_side` flips where the very first page sits
/// (`--invertdirection`, `--spreadshift`, or both cancelling out);
/// `one_page_landscape` overrides everything with "center".
fn page_spread_sides(
    roles: &[PageRole],
    right_to_left: bool,
    start_on_second_side: bool,
    one_page_landscape: bool,
) -> Vec<&'static str> {
    if one_page_landscape {
        return vec!["center"; roles.len()];
    }
    let (first_side, second_side) = if right_to_left {
        ("right", "left")
    } else {
        ("left", "right")
    };
    let flip = |side: &'static str| if side == "right" { "left" } else { "right" };

    let mut sides = Vec::with_capacity(roles.len());
    let mut side = if start_on_second_side {
        second_side
    } else {
        first_side
    };
    for role in roles {
        match role {
            PageRole::Normal => {
                sides.push(side);
                side = flip(side);
            }
            PageRole::SplitFirst => {
                sides.push(first_side);
                side = first_side;
            }
            PageRole::SplitSecond => {
                sides.push(second_side);
                side = first_side;
            }
            PageRole::Rotated => {
                sides.push("center");
                side = first_side;
            }
        }
    }

    let mut spread_seen = false;
    for (index, role) in roles.iter().enumerate().rev() {
        if *role != PageRole::Normal {
            spread_seen = true;
            side = second_side;
        } else if spread_seen {
            sides[index] = side;
            side = flip(side);
        }
    }
    sides
}

pub fn build_epub(chapters: &[Chapter], options: &EpubOptions) -> Result<Vec<u8>> {
    let chapters: Vec<&Chapter> = chapters.iter().filter(|c| !c.pages.is_empty()).collect();
    if chapters.is_empty() {
        return Err(Error::EmptyBook);
    }

    // Hashing only the title (as an earlier version of this function did)
    // gives two different books sharing a title the exact same
    // `dc:identifier` -- widen the seed with author and per-chapter titles
    // so distinct books collide only in the (still not cryptographically
    // guaranteed, but now far less likely) case that all of those also
    // match.
    let identifier_seed = format!(
        "{}\u{0}{}\u{0}{}",
        options.title,
        // Joined the way the single author field used to arrive, so a book
        // keeps the identifier it had before authors were kept apart.
        options.authors.join(", "),
        chapters
            .iter()
            .map(|c| c.title.as_str())
            .collect::<Vec<_>>()
            .join("\u{0}")
    );
    let identifier = synthetic_identifier(&identifier_seed);

    // EPUB OCF requires `mimetype` to be the first entry and stored
    // uncompressed; everything else in this ZIP is stored uncompressed too
    // (see write_zip's call at the bottom) — text/XML overhead is tiny next
    // to already-compressed page images, so this trades a little size for
    // one less thing to get wrong.
    let mut zip_entries: Vec<(String, Vec<u8>)> = vec![
        ("mimetype".to_string(), b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml".to_string(),
            build_container_xml().into_bytes(),
        ),
    ];

    let mut manifest_items = Vec::new();
    let mut page_roles = Vec::new();
    let mut chapter_hrefs = Vec::with_capacity(chapters.len());
    // For bookmarks: every page's own href, and where each *source* page's
    // first output page sits in that list.
    let mut page_hrefs = Vec::new();
    let mut source_page_starts = Vec::new();

    if let Some(cover) = &options.cover {
        zip_entries.push(("OEBPS/Images/cover.jpg".to_string(), cover.clone()));
        manifest_items.push(
            r#"<item id="cover" href="Images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#
                .to_string(),
        );
    }

    let mut page_counter = 0usize;
    for (chapter_index, chapter) in chapters.iter().enumerate() {
        let mut first_href: Option<String> = None;

        for (page_index, page) in chapter.pages.iter().enumerate() {
            page_counter += 1;
            // Read just the encoded header, not a full pixel decode -- this
            // only needs the two dimensions for the XHTML viewport meta tag.
            let (width, height) = image::ImageReader::new(std::io::Cursor::new(&page.bytes))
                .with_guessed_format()
                .ok()
                .and_then(|reader| reader.into_dimensions().ok())
                .unwrap_or((0, 0));

            let dir = format!("c{:04}", chapter_index + 1);
            let file_stem = format!("p{:04}", page_index + 1);
            let image_path = format!("Images/{dir}/{file_stem}.{}", page.extension);
            let xhtml_path = format!("Text/{dir}/{file_stem}.xhtml");
            let media_type =
                media_type_for_extension(&page.extension).unwrap_or("application/octet-stream");

            zip_entries.push((format!("OEBPS/{image_path}"), page.bytes.clone()));
            let xhtml = build_page_xhtml(
                &format!("{} - page {page_counter}", options.title),
                &image_path,
                width,
                height,
                page.black_background,
                options.kindle,
            );
            zip_entries.push((format!("OEBPS/{xhtml_path}"), xhtml.into_bytes()));

            // Without a cover of its own (see `EpubOptions::cover`), the
            // book's very first page doubles as one: tagged
            // `properties="cover-image"` (the EPUB3 way) and referenced by
            // `<meta name="cover">` in the OPF metadata (the older EPUB2
            // convention many readers still look for).
            let cover_property = if page_counter == 1 && options.cover.is_none() {
                r#" properties="cover-image""#
            } else {
                ""
            };
            manifest_items.push(format!(
                r#"<item id="img{page_counter}" href="{image_path}" media-type="{media_type}"{cover_property}/>"#
            ));
            manifest_items.push(format!(
                r#"<item id="page{page_counter}" href="{xhtml_path}" media-type="application/xhtml+xml"/>"#
            ));
            page_roles.push(page.role);
            if !page.continues_source_page {
                source_page_starts.push(page_hrefs.len());
            }
            page_hrefs.push(xhtml_path.clone());

            first_href.get_or_insert_with(|| xhtml_path.clone());
        }

        chapter_hrefs.push(first_href.expect("chapters with zero pages were filtered out above"));
    }

    let sides = page_spread_sides(
        &page_roles,
        options.reading_direction.right_to_left,
        options.invert_direction != options.spread_shift,
        options.one_page_landscape,
    );
    let spine_itemrefs: Vec<String> = sides
        .iter()
        .enumerate()
        .map(|(index, side)| {
            let page_number = index + 1;
            // EPUB defines bare left/right properties, but no bare center
            // property. Preserve Kindle's older spelling only where valid.
            let prefix = if options.kindle && *side != "center" {
                ""
            } else {
                "rendition:"
            };
            let linear = if options.kindle { r#" linear="yes""# } else { "" };
            format!(
                r#"<itemref idref="page{page_number}"{linear} properties="{prefix}page-spread-{side}"/>"#
            )
        })
        .collect();

    // What the table of contents lists: the bookmarks if the book has any
    // that name a real page, else the folders.
    let bookmarked = bookmark_toc(&options.bookmarks, &source_page_starts, &page_hrefs);
    let bookmark_chapters: Vec<&Chapter> = bookmarked.iter().map(|(chapter, _)| chapter).collect();
    let bookmark_hrefs: Vec<String> = bookmarked.iter().map(|(_, href)| href.clone()).collect();
    let (toc_chapters, toc_hrefs, nested): (&[&Chapter], &[String], bool) =
        if bookmark_chapters.is_empty() {
            (&chapters, &chapter_hrefs, options.nested_toc)
        } else {
            (&bookmark_chapters, &bookmark_hrefs, false)
        };

    zip_entries.push((
        "OEBPS/content.opf".to_string(),
        build_opf(options, &identifier, &manifest_items, &spine_itemrefs).into_bytes(),
    ));
    zip_entries.push((
        "OEBPS/toc.ncx".to_string(),
        build_ncx(options, &identifier, toc_chapters, toc_hrefs, nested).into_bytes(),
    ));
    zip_entries.push((
        "OEBPS/nav.xhtml".to_string(),
        build_nav(options, toc_chapters, toc_hrefs, nested).into_bytes(),
    ));

    crate::archive::cbz::write_zip(&zip_entries, true)
}

fn build_container_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles>
<rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
</rootfiles>
</container>"#
        .to_string()
}

fn build_opf(
    options: &EpubOptions,
    identifier: &str,
    manifest_items: &[String],
    spine_itemrefs: &[String],
) -> String {
    let creators: String = options
        .authors
        .iter()
        .map(|author| format!("<dc:creator>{}</dc:creator>\n", xml_escape(author)))
        .collect();
    // Upstream's Kindle-only block: what tells Kindle's own renderer this is
    // a fixed-layout comic at the device's resolution, read in this
    // direction, with no gutter or margin of its own.
    let kindle_metas = match options.kindle_resolution {
        Some((width, height)) => format!(
            // `r##`: the border color's `"#` would end an `r#` string.
            r##"<meta name="fixed-layout" content="true"/>
<meta name="original-resolution" content="{width}x{height}"/>
<meta name="book-type" content="comic"/>
<meta name="primary-writing-mode" content="{writing_mode}"/>
<meta name="zero-gutter" content="true"/>
<meta name="zero-margin" content="true"/>
<meta name="ke-border-color" content="#FFFFFF"/>
<meta name="ke-border-width" content="0"/>
<meta name="orientation-lock" content="none"/>
<meta name="region-mag" content="true"/>
"##,
            writing_mode = if options.reading_direction.right_to_left != options.invert_direction {
                "horizontal-rl"
            } else {
                "horizontal-lr"
            },
        ),
        None => String::new(),
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="{lang}">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="pub-id">{identifier}</dc:identifier>
<dc:title>{title}</dc:title>
{creators}<dc:language>{lang}</dc:language>
<dc:contributor id="contributor">mangapress-{version}</dc:contributor>
{description}{series}<meta property="dcterms:modified">{modified}</meta>
<meta name="cover" content="{cover_id}"/>
{kindle_metas}<meta property="rendition:spread">landscape</meta>
<meta property="rendition:layout">pre-paginated</meta>
</metadata>
<manifest>
<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
{items}
</manifest>
<spine toc="ncx" page-progression-direction="{direction}">
{spine}
</spine>
</package>"#,
        lang = xml_escape(&options.language),
        identifier = xml_escape(identifier),
        title = xml_escape(&options.title),
        version = env!("CARGO_PKG_VERSION"),
        cover_id = if options.cover.is_some() {
            "cover"
        } else {
            "img1"
        },
        series = match (&options.series, options.kindle) {
            (Some((name, position)), false) => format!(
                // `r##`: `"#c02"` would end an `r#` string.
                r##"<meta property="belongs-to-collection" id="c02">{name}</meta>
<meta refines="#c02" property="collection-type">series</meta>
{position}"##,
                name = xml_escape(name),
                position = position
                    .as_deref()
                    .map(|p| format!(
                        "<meta refines=\"#c02\" property=\"group-position\">{}</meta>\n",
                        xml_escape(p)
                    ))
                    .unwrap_or_default(),
            ),
            _ => String::new(),
        },
        modified = xml_escape(&options.modified),
        description = options
            .description
            .as_deref()
            .map(|d| format!("<dc:description>{}</dc:description>\n", xml_escape(d)))
            .unwrap_or_default(),
        items = manifest_items.join("\n"),
        direction = if options.reading_direction.right_to_left != options.invert_direction {
            "rtl"
        } else {
            "ltr"
        },
        spine = spine_itemrefs.join("\n"),
    )
}

/// One page's XHTML. Upstream reference: `buildHTML()` in upstream's
/// `comic2ebook.py` — the image is an *inline* `<img>` carrying its own pixel
/// size as `width`/`height` attributes, inside a `text-align:center` block,
/// under the same two stylesheet rules upstream ships in its `style.css`.
///
/// The image is deliberately never sized in percentages, and never made
/// `display: block`. An earlier version of this function wrote `img {
/// display: block; width: 100%; height: 100%; }` instead, which is harmless
/// in a reader that honors the fixed-layout viewport (the viewport is the
/// image's own size, so 100% of it changes nothing) but not in KOReader:
/// crengine ignores that viewport, lays the page out against the screen, and
/// sizes a block image's two axes independently. Confirmed by rendering a
/// real volume through KOReader's own crengine at 1072x1448, not assumed: a
/// 965x1448 page came out 1072x1448, stretched 11% sideways (about 15% with
/// KOReader's default margins and status bar), where upstream's markup for
/// the same page kept its proportions. The two obvious half-measures were
/// rendered the same way and both still stretch — `width: 100%; height:
/// auto` by the same 11-15%, and a `display: block` image with fixed
/// `width`/`height` by 7% once margins leave it less than its own height —
/// so this follows upstream's markup as a whole rather than patching the one
/// property that first looked wrong.
///
/// `width`/`height` are `0` when the page's header couldn't be read (see the
/// caller): the viewport and the image's size attributes are then both left
/// out, and the image falls back to its intrinsic size.
///
/// Two more things upstream writes, both carried over: a dark page gets a
/// black background on its `<body>` (so the strip a centered page leaves
/// beside it is black like the page, not white), and a Kindle profile gets
/// a hidden, non-empty first block inside the centered one, which upstream
/// adds because Kindle's own renderer misplaces the page without it.
fn build_page_xhtml(
    title: &str,
    image_path_from_oebps: &str,
    width: u32,
    height: u32,
    black_background: bool,
    kindle: bool,
) -> String {
    // Text/cNNNN/pNNNN.xhtml -> Images/cNNNN/pNNNN.ext is always two levels
    // up then back down, given the fixed directory layout above.
    let image_href = format!("../../{image_path_from_oebps}");
    let (viewport, image_size) = if width > 0 && height > 0 {
        (
            format!(r#"<meta name="viewport" content="width={width}, height={height}"/>"#),
            format!(r#" width="{width}" height="{height}""#),
        )
    } else {
        (String::new(), String::new())
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
<title>{title}</title>
<meta charset="utf-8"/>
{viewport}
<style type="text/css">
@page {{ margin: 0; }}
body {{ display: block; margin: 0; padding: 0; }}
</style>
</head>
<body{body_style}>
<div style="text-align:center;">
{kindle_block}<img{image_size} src="{image_href}" alt=""/>
</div>
</body>
</html>"#,
        title = xml_escape(title),
        body_style = if black_background {
            r#" style="background-color:#000000;""#
        } else {
            ""
        },
        kindle_block = if kindle {
            "<div style=\"display:none;\">.</div>\n"
        } else {
            ""
        },
    )
}

fn build_ncx(
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

fn build_nav(
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
/// for this one more level). No change to [`super::group_into_chapters`] or
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

fn media_type_for_extension(ext: &str) -> Option<&'static str> {
    match ext {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
fn bookmark_toc(
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

/// The publication's identifier: a name-based UUID (RFC 4122 version 5)
/// over the book's title, authors and chapter titles, in upstream's own
/// `urn:uuid:` form.
///
/// Upstream draws a random UUID per conversion. This derives it instead, so
/// converting the same book again gives the same identifier — which is what
/// EPUB 3 asks of one: the identifier names the publication and stays put
/// across its releases, and `dcterms:modified` tells the releases apart.
///
/// Version 5 rather than a hash of our own choosing because it is specified
/// bit for bit. An earlier version formatted `std`'s `DefaultHasher` output,
/// whose algorithm the standard library reserves the right to change in any
/// release: the "stable" identifier was only stable until the next compiler
/// that changed it.
fn synthetic_identifier(seed: &str) -> String {
    let id = uuid_v5(&MANGAPRESS_NAMESPACE, seed.as_bytes());
    let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// RFC 4122's namespace for names that are URLs.
#[cfg(test)]
const URL_NAMESPACE: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

/// The namespace book identifiers are derived in: the version 5 UUID of
/// this project's own URL, `https://github.com/gustavommcv/mangapress`, in
/// the URL namespace — a3433aff-4bf2-51ee-a1c9-836d85be953f. Fixed forever:
/// changing it changes every book's identifier.
const MANGAPRESS_NAMESPACE: [u8; 16] = [
    0xa3, 0x43, 0x3a, 0xff, 0x4b, 0xf2, 0x51, 0xee, 0xa1, 0xc9, 0x83, 0x6d, 0x85, 0xbe, 0x95, 0x3f,
];

/// RFC 4122 version 5: SHA-1 of the namespace followed by the name, cut to
/// 16 bytes, with the version and variant bits set.
fn uuid_v5(namespace: &[u8; 16], name: &[u8]) -> [u8; 16] {
    let mut input = namespace.to_vec();
    input.extend_from_slice(name);
    let digest = sha1(&input);

    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id[6] = (id[6] & 0x0f) | 0x50;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// SHA-1 (FIPS 180-4), for [`uuid_v5`] only — an identifier, not a security
/// boundary. Written out here rather than pulled in as a dependency for one
/// 20-byte digest per book.
fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];

    // The message, a single 1 bit, zeros up to 8 bytes short of a 64-byte
    // block, then the message's length in bits.
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 20];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    digest
}

#[cfg(test)]
#[path = "epub/tests.rs"]
mod tests;
