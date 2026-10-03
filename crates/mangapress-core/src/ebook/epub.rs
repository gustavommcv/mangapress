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
    /// carries upstream's hidden first block, and spine items are tagged
    /// `page-spread-*` the Kindle way rather than `rendition:page-spread-*`.
    pub kindle: bool,
    /// The device resolution, for a Kindle profile used at its own
    /// resolution — writes upstream's Kindle fixed-layout `<meta>` block,
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
            if options.kindle {
                format!(
                    r#"<itemref idref="page{page_number}" linear="yes" properties="page-spread-{side}"/>"#
                )
            } else {
                format!(
                    r#"<itemref idref="page{page_number}" properties="rendition:page-spread-{side}"/>"#
                )
            }
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
    let mut order = 0usize;
    let nav_points: String = if nested {
        group_by_volume(chapters, chapter_hrefs)
            .into_iter()
            .map(|volume| {
                order += 1;
                let volume_order = order;
                let children: String = volume
                    .chapters
                    .iter()
                    .map(|(chapter, href)| {
                        order += 1;
                        format!(
                            r#"<navPoint id="chapter{order}" playOrder="{order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/></navPoint>"#,
                            title = xml_escape(&chapter.title),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    r#"<navPoint id="volume{volume_order}" playOrder="{volume_order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/>
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
                order += 1;
                format!(
                    r#"<navPoint id="chapter{order}" playOrder="{order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/></navPoint>"#,
                    title = xml_escape(&chapter.title),
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
<meta name="dtb:depth" content="1"/>
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
                            title = xml_escape(&chapter.title),
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
                    title = xml_escape(&chapter.title),
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
                    title = xml_escape(&chapter.title),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
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
mod tests {
    use super::*;
    use crate::ebook::Page;
    use std::path::PathBuf;

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(4, 6, image::Rgb([255, 0, 0]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    fn sample_chapters() -> Vec<Chapter> {
        vec![
            Chapter {
                relative_path: PathBuf::from("c001 - Title One"),
                title: "c001 - Title One".to_string(),
                pages: vec![
                    Page {
                        extension: "png".to_string(),
                        bytes: tiny_png(),
                        ..Default::default()
                    },
                    Page {
                        extension: "png".to_string(),
                        bytes: tiny_png(),
                        ..Default::default()
                    },
                ],
            },
            Chapter {
                relative_path: PathBuf::from("c002 - Nyako's Whereabouts"),
                title: "c002 - Nyako's Whereabouts".to_string(),
                pages: vec![Page {
                    extension: "png".to_string(),
                    bytes: tiny_png(),
                    ..Default::default()
                }],
            },
        ]
    }

    fn default_options() -> EpubOptions {
        EpubOptions {
            title: "Test Book".to_string(),
            authors: vec!["Test Author".to_string()],
            language: "en".to_string(),
            reading_direction: ReadingDirection {
                right_to_left: true,
            },
            description: None,
            nested_toc: false,
            kindle: false,
            kindle_resolution: None,
            invert_direction: false,
            spread_shift: false,
            one_page_landscape: false,
            cover: None,
            bookmarks: Vec::new(),
            series: None,
            modified: "2026-01-02T03:04:05Z".to_string(),
        }
    }

    #[test]
    fn empty_book_is_rejected() {
        let result = build_epub(&[], &default_options());
        assert!(matches!(result, Err(Error::EmptyBook)));
    }

    #[test]
    fn produces_a_valid_zip_with_mimetype_first_and_stored() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mimetype_entry = archive.by_index(0).unwrap();
        assert_eq!(mimetype_entry.name(), "mimetype");
        assert_eq!(mimetype_entry.compression(), zip::CompressionMethod::Stored);
    }

    #[test]
    fn mimetype_contents_are_exact() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut entry = archive.by_name("mimetype").unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut entry, &mut contents).unwrap();
        assert_eq!(contents, "application/epub+zip");
    }

    #[test]
    fn toc_and_nav_list_every_chapter_by_title() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

        let mut ncx = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/toc.ncx").unwrap(), &mut ncx)
            .unwrap();
        assert!(ncx.contains("c001 - Title One"));
        // The apostrophe must survive XML-escaped, not mangled or dropped.
        assert!(ncx.contains("c002 - Nyako&apos;s Whereabouts"));
        assert_eq!(ncx.matches("<navPoint").count(), 2);

        let mut nav = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/nav.xhtml").unwrap(), &mut nav)
            .unwrap();
        assert!(nav.contains("c001 - Title One"));
        assert!(nav.contains("c002 - Nyako&apos;s Whereabouts"));
    }

    #[test]
    fn the_first_page_is_declared_as_the_book_cover() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut opf = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/content.opf").unwrap(), &mut opf)
            .unwrap();
        assert!(opf.contains(r#"<meta name="cover" content="img1"/>"#));
        assert!(opf.contains(r#"id="img1" href="Images/c0001/p0001.png" media-type="image/png" properties="cover-image"/>"#));
        // No other page's manifest item should carry the cover property.
        assert_eq!(opf.matches("cover-image").count(), 1);
    }

    #[test]
    fn manifest_and_spine_cover_every_page() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut opf = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/content.opf").unwrap(), &mut opf)
            .unwrap();
        // 3 total pages across both chapters.
        assert_eq!(opf.matches("<itemref").count(), 3);
        assert_eq!(opf.matches("image/png").count(), 3);
        assert!(opf.contains(r#"page-progression-direction="rtl""#));
        assert!(opf.contains("pre-paginated"));
    }

    fn read_entry(epub: Vec<u8>, name: &str) -> String {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(epub)).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(name).unwrap(), &mut contents).unwrap();
        contents
    }

    #[test]
    fn a_page_image_carries_its_own_pixel_size_inside_a_centered_block() {
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let page = read_entry(bytes, "OEBPS/Text/c0001/p0001.xhtml");
        // `tiny_png()` is 4x6.
        assert!(page.contains(r#"<meta name="viewport" content="width=4, height=6"/>"#));
        assert!(page.contains(
            r#"<div style="text-align:center;">
<img width="4" height="6" src="../../Images/c0001/p0001.png" alt=""/>
</div>"#
        ));
    }

    #[test]
    fn a_page_image_is_never_sized_in_percentages_or_made_a_block() {
        // The regression this guards: `img { display: block; width: 100%;
        // height: 100%; }` is stretched sideways by KOReader's crengine,
        // which sizes a block image's axes independently against the screen
        // (see `build_page_xhtml`'s docs for the measurements).
        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let page = read_entry(bytes, "OEBPS/Text/c0001/p0001.xhtml");
        assert!(!page.contains('%'), "no percentage sizing anywhere: {page}");
        assert!(
            !page.contains("img {"),
            "no stylesheet rule targets the image: {page}"
        );
    }

    #[test]
    fn a_page_whose_size_cannot_be_read_leaves_the_image_at_its_intrinsic_size() {
        let chapters = vec![Chapter {
            relative_path: PathBuf::from("c001"),
            title: "c001".to_string(),
            pages: vec![Page {
                extension: "jpg".to_string(),
                bytes: b"not an image".to_vec(),
                ..Default::default()
            }],
        }];
        let bytes = build_epub(&chapters, &default_options()).unwrap();
        let page = read_entry(bytes, "OEBPS/Text/c0001/p0001.xhtml");
        assert!(!page.contains("viewport"));
        assert!(page.contains(r#"<img src="../../Images/c0001/p0001.jpg" alt=""/>"#));
    }

    #[test]
    fn description_is_included_when_present_and_omitted_when_absent() {
        let mut with_description = default_options();
        with_description.description = Some("A summary & more".to_string());
        let bytes = build_epub(&sample_chapters(), &with_description).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut opf = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/content.opf").unwrap(), &mut opf)
            .unwrap();
        assert!(opf.contains("<dc:description>A summary &amp; more</dc:description>"));

        let bytes = build_epub(&sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut opf = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/content.opf").unwrap(), &mut opf)
            .unwrap();
        assert!(!opf.contains("dc:description"));
    }

    #[test]
    fn xml_escape_handles_every_special_character() {
        assert_eq!(
            xml_escape("A & B <C> \"D\" 'E'"),
            "A &amp; B &lt;C&gt; &quot;D&quot; &apos;E&apos;"
        );
    }

    /// A chapter, keyed by a two-level "<volume>/<chapter>" relative path -
    /// what group_into_chapters produces from a Mangabind `-combine` output
    /// (see docs/adr/0012-nested-toc-for-combined-volumes.md). The chapter's
    /// own title is just its own directory name, matching the real pipeline
    /// (group_into_chapters titles a chapter from relative_path.file_name(),
    /// never the full path).
    fn nested_chapter(volume: &str, chapter_dir: &str) -> Chapter {
        Chapter {
            relative_path: PathBuf::from(volume).join(chapter_dir),
            title: chapter_dir.to_string(),
            pages: vec![Page {
                extension: "png".to_string(),
                bytes: tiny_png(),
                ..Default::default()
            }],
        }
    }

    fn nested_sample_chapters() -> Vec<Chapter> {
        vec![
            nested_chapter("v001 - Vol.01", "c001 - Alpha"),
            nested_chapter("v001 - Vol.01", "c002 - Beta"),
            nested_chapter("v002 - Vol.02", "c001 - Gamma"),
        ]
    }

    #[test]
    fn nested_toc_groups_chapters_under_their_volume() {
        let mut options = default_options();
        options.nested_toc = true;
        let bytes = build_epub(&nested_sample_chapters(), &options).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

        let mut ncx = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/toc.ncx").unwrap(), &mut ncx)
            .unwrap();
        // Two volumes at the top level, three chapters total nested inside them.
        assert_eq!(ncx.matches("<navPoint id=\"volume").count(), 2);
        assert_eq!(ncx.matches("<navPoint id=\"chapter").count(), 3);
        assert!(ncx.contains("v001 - Vol.01"));
        assert!(ncx.contains("v002 - Vol.02"));
        assert!(ncx.contains("c001 - Alpha"));
        assert!(ncx.contains("c002 - Beta"));
        assert!(ncx.contains("c001 - Gamma"));
        // Volume 1's navPoint must actually contain (not just precede) its
        // two chapters' navPoints - this is the real regression this
        // feature is for, not just "every label shows up somewhere". Ids
        // aren't sequential per type (volume1, volume2, ...): playOrder is
        // one shared, strictly sequential counter across every navPoint,
        // parent and child alike, as NCX requires - so the second volume's
        // id is whatever order it lands on, not necessarily "volume2".
        let volume_starts: Vec<usize> = ncx
            .match_indices("<navPoint id=\"volume")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(volume_starts.len(), 2);
        let volume1_block = &ncx[volume_starts[0]..volume_starts[1]];
        assert!(volume1_block.contains("c001 - Alpha"));
        assert!(volume1_block.contains("c002 - Beta"));
        assert!(
            !volume1_block.contains("c001 - Gamma"),
            "volume 1's chapters must not include volume 2's"
        );

        let mut nav = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/nav.xhtml").unwrap(), &mut nav)
            .unwrap();
        // A nested <ol> inside a volume's own <li> is what gives KOReader (and
        // any EPUB3-nav-aware reader) an expandable volume/chapter tree: the
        // one outer <ol> (the whole TOC) plus one more per volume.
        // Only the table of contents nests; the page list after it is flat.
        let toc = nav.split(r#"<nav epub:type="page-list">"#).next().unwrap();
        assert_eq!(
            toc.matches("<ol>").count(),
            3,
            "the outer <ol> plus one nested <ol> per volume"
        );
        assert_eq!(nav.matches("<ol>").count(), 4, "plus the page list's own");
        assert!(nav.contains("v001 - Vol.01"));
        assert!(nav.contains("v002 - Vol.02"));
    }

    #[test]
    fn nested_toc_false_stays_flat_even_with_nested_relative_paths() {
        // Proves nested_toc is a strict opt-in: chapters that happen to have
        // a nested relative_path (for whatever reason) still produce today's
        // plain flat list when the flag is off, matching existing behavior
        // byte-for-byte for anyone not using Mangabind's -combine output.
        let bytes = build_epub(&nested_sample_chapters(), &default_options()).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

        let mut ncx = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("OEBPS/toc.ncx").unwrap(), &mut ncx)
            .unwrap();
        assert_eq!(ncx.matches("<navPoint id=\"chapter").count(), 3);
        assert_eq!(
            ncx.matches("<navPoint id=\"volume").count(),
            0,
            "no volume grouping when nested_toc is off"
        );
    }

    fn roles(spec: &str) -> Vec<PageRole> {
        spec.split_whitespace()
            .map(|role| match role {
                "N" => PageRole::Normal,
                "S1" => PageRole::SplitFirst,
                "S2" => PageRole::SplitSecond,
                "R" => PageRole::Rotated,
                other => panic!("unknown role {other}"),
            })
            .collect()
    }

    #[test]
    fn page_spread_sides_match_upstream() {
        // Every expectation is what KCC 12.0.0's own `buildOPF()` wrote for
        // the same sequence of pages (N ordinary, S1/S2 the halves of a
        // split spread, R a rotated spread).
        for (pages, right_to_left, expected) in [
            ("N N N N", true, "right left right left"),
            ("N N N N", false, "left right left right"),
            (
                "N N N S1 S2 R N N",
                true,
                "left right left right left center right left",
            ),
            (
                "N N N S1 S2 R N N",
                false,
                "right left right left right center left right",
            ),
            (
                "N S1 S2 N N R N",
                true,
                "left right left right left center right",
            ),
            (
                "N S1 S2 N N R N",
                false,
                "right left right left right center left",
            ),
            ("R N N N", true, "center right left right"),
            ("R N N N", false, "center left right left"),
            ("N N S1 S2", true, "right left right left"),
            ("N N S1 S2", false, "left right left right"),
            ("N N N N N R", true, "left right left right left center"),
            ("N N N N N R", false, "right left right left right center"),
        ] {
            assert_eq!(
                page_spread_sides(&roles(pages), right_to_left, false, false).join(" "),
                expected,
                "{pages}, right_to_left={right_to_left}"
            );
        }
    }

    #[test]
    fn spine_items_carry_their_page_side_in_the_profile_familys_own_form() {
        let opf = read_entry(
            build_epub(&sample_chapters(), &default_options()).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(
            opf.contains(r#"<itemref idref="page1" properties="rendition:page-spread-right"/>"#)
        );
        assert!(opf.contains(r#"<itemref idref="page2" properties="rendition:page-spread-left"/>"#));

        let mut kindle = default_options();
        kindle.kindle = true;
        let opf = read_entry(
            build_epub(&sample_chapters(), &kindle).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(
            opf.contains(r#"<itemref idref="page1" linear="yes" properties="page-spread-right"/>"#)
        );
    }

    #[test]
    fn kindle_fixed_layout_metadata_needs_a_kindle_resolution() {
        let opf = read_entry(
            build_epub(&sample_chapters(), &default_options()).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(!opf.contains("original-resolution"));
        assert!(!opf.contains("fixed-layout"));

        let mut kindle = default_options();
        kindle.kindle = true;
        kindle.kindle_resolution = Some((1072, 1448));
        let opf = read_entry(
            build_epub(&sample_chapters(), &kindle).unwrap(),
            "OEBPS/content.opf",
        );
        for meta in [
            r#"<meta name="fixed-layout" content="true"/>"#,
            r#"<meta name="original-resolution" content="1072x1448"/>"#,
            r#"<meta name="book-type" content="comic"/>"#,
            r#"<meta name="primary-writing-mode" content="horizontal-rl"/>"#,
            r#"<meta name="zero-gutter" content="true"/>"#,
            r#"<meta name="zero-margin" content="true"/>"#,
            r##"<meta name="ke-border-color" content="#FFFFFF"/>"##,
            r#"<meta name="ke-border-width" content="0"/>"#,
            r#"<meta name="orientation-lock" content="none"/>"#,
            r#"<meta name="region-mag" content="true"/>"#,
        ] {
            assert!(opf.contains(meta), "missing {meta}");
        }
    }

    #[test]
    fn package_metadata_names_every_author_the_generator_and_when_it_was_built() {
        let mut options = default_options();
        options.authors = vec!["First Author".to_string(), "Second & Co".to_string()];
        let opf = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(opf.contains(
            "<dc:creator>First Author</dc:creator>\n<dc:creator>Second &amp; Co</dc:creator>\n"
        ));
        assert!(opf.contains(concat!(
            r#"<dc:contributor id="contributor">mangapress-"#,
            env!("CARGO_PKG_VERSION"),
            "</dc:contributor>"
        )));
        assert!(opf.contains(r#"<meta property="dcterms:modified">2026-01-02T03:04:05Z</meta>"#));
        assert!(opf.contains(r#"<meta property="rendition:spread">landscape</meta>"#));
    }

    #[test]
    fn series_metadata_is_written_for_every_profile_but_a_kindle_one() {
        let mut options = default_options();
        options.series = Some(("A & B".to_string(), Some("2.5".to_string())));
        let opf = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(opf.contains(concat!(
            r##"<meta property="belongs-to-collection" id="c02">A &amp; B</meta>"##,
            "\n",
            r##"<meta refines="#c02" property="collection-type">series</meta>"##,
            "\n",
            r##"<meta refines="#c02" property="group-position">2.5</meta>"##,
            "\n",
        )));

        options.series = Some(("A & B".to_string(), None));
        let opf = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(opf.contains("belongs-to-collection"));
        assert!(!opf.contains("group-position"));

        options.kindle = true;
        let opf = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(!opf.contains("belongs-to-collection"));
    }

    #[test]
    fn a_dark_page_gets_a_black_page_background() {
        let mut chapters = sample_chapters();
        chapters[0].pages[1].black_background = true;
        let bytes = build_epub(&chapters, &default_options()).unwrap();
        assert!(read_entry(bytes.clone(), "OEBPS/Text/c0001/p0002.xhtml")
            .contains(r#"<body style="background-color:#000000;">"#));
        assert!(read_entry(bytes, "OEBPS/Text/c0001/p0001.xhtml").contains("<body>\n"));
    }

    #[test]
    fn a_kindle_profile_page_starts_with_upstreams_hidden_block() {
        let mut kindle = default_options();
        kindle.kindle = true;
        let page = read_entry(
            build_epub(&sample_chapters(), &kindle).unwrap(),
            "OEBPS/Text/c0001/p0001.xhtml",
        );
        assert!(page.contains(
            "<div style=\"text-align:center;\">\n<div style=\"display:none;\">.</div>\n<img "
        ));
        let page = read_entry(
            build_epub(&sample_chapters(), &default_options()).unwrap(),
            "OEBPS/Text/c0001/p0001.xhtml",
        );
        assert!(!page.contains("display:none"));
    }

    #[test]
    fn nav_lists_the_chapters_again_as_a_page_list() {
        let nav = read_entry(
            build_epub(&sample_chapters(), &default_options()).unwrap(),
            "OEBPS/nav.xhtml",
        );
        let page_list = nav
            .split(r#"<nav epub:type="page-list">"#)
            .nth(1)
            .expect("a page-list nav after the table of contents");
        assert_eq!(page_list.matches("<li>").count(), 2);
        assert!(
            page_list.contains(r#"<li><a href="Text/c0001/p0001.xhtml">c001 - Title One</a></li>"#)
        );
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn sha1_matches_the_published_test_vectors() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn uuid_v5_matches_the_reference_implementation() {
        // Python's documented example: uuid.uuid5(uuid.NAMESPACE_DNS, 'python.org').
        let dns_namespace = [
            0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4,
            0x30, 0xc8,
        ];
        assert_eq!(
            hex(&uuid_v5(&dns_namespace, b"python.org")),
            "886313e13b8a53729b900c9aee199e5d"
        );
        // A name long enough to need more than one SHA-1 block.
        assert_eq!(
            hex(&uuid_v5(&MANGAPRESS_NAMESPACE, "x".repeat(200).as_bytes())),
            "1099b88999165bac819e3b04bba03f15"
        );
    }

    #[test]
    fn the_namespace_is_this_projects_url_in_the_url_namespace() {
        assert_eq!(
            uuid_v5(&URL_NAMESPACE, b"https://github.com/gustavommcv/mangapress"),
            MANGAPRESS_NAMESPACE
        );
    }

    #[test]
    fn the_identifier_is_a_uuid_derived_from_the_book_and_never_changes() {
        // uuid.uuid5(namespace, 'Test Book\0Test Author\0c001 - Title One\0c002 - Nyako's Whereabouts')
        // in Python. Pinned: this value changing means every existing book's
        // identifier changed with it.
        let opf = read_entry(
            build_epub(&sample_chapters(), &default_options()).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(opf.contains(
            r#"<dc:identifier id="pub-id">urn:uuid:79c06941-cb22-5f17-87ae-56e276f3d1a9</dc:identifier>"#
        ));

        let mut other = default_options();
        other.title = "Another Book".to_string();
        let opf = read_entry(
            build_epub(&sample_chapters(), &other).unwrap(),
            "OEBPS/content.opf",
        );
        assert!(!opf.contains("79c06941-cb22-5f17-87ae-56e276f3d1a9"));
    }

    #[test]
    fn a_separate_cover_is_written_and_declared_instead_of_the_first_page() {
        let mut options = default_options();
        options.cover = Some(b"cover bytes".to_vec());
        let bytes = build_epub(&sample_chapters(), &options).unwrap();

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
        let mut cover = Vec::new();
        std::io::Read::read_to_end(
            &mut archive.by_name("OEBPS/Images/cover.jpg").unwrap(),
            &mut cover,
        )
        .unwrap();
        assert_eq!(cover, b"cover bytes");

        let opf = read_entry(bytes, "OEBPS/content.opf");
        assert!(opf.contains(
            r#"<item id="cover" href="Images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#
        ));
        assert!(opf.contains(r#"<meta name="cover" content="cover"/>"#));
        assert!(opf
            .contains(r#"<item id="img1" href="Images/c0001/p0001.png" media-type="image/png"/>"#));
        assert_eq!(opf.matches("cover-image").count(), 1);
    }

    #[test]
    fn bookmarks_replace_the_folder_table_of_contents() {
        let mut options = default_options();
        options.bookmarks = vec![
            (0, "Opening".to_string()),
            (2, "Second half & more".to_string()),
            (99, "Past the last page".to_string()),
        ];
        let bytes = build_epub(&sample_chapters(), &options).unwrap();

        let nav = read_entry(bytes.clone(), "OEBPS/nav.xhtml");
        let toc = nav.split(r#"<nav epub:type="page-list">"#).next().unwrap();
        assert!(toc.contains(r#"<li><a href="Text/c0001/p0001.xhtml">Opening</a></li>"#));
        // Source page 2 is the first page of the second folder.
        assert!(
            toc.contains(r#"<li><a href="Text/c0002/p0001.xhtml">Second half &amp; more</a></li>"#)
        );
        assert_eq!(toc.matches("<li>").count(), 2, "{toc}");
        assert!(!nav.contains("c001 - Title One"));
        assert!(!nav.contains("Past the last page"));

        let ncx = read_entry(bytes, "OEBPS/toc.ncx");
        assert_eq!(ncx.matches("<navPoint").count(), 2);
        assert!(ncx.contains("<text>Opening</text>"));
    }

    #[test]
    fn a_bookmark_counts_source_pages_not_the_pages_a_spread_became() {
        // Source pages: 0 ordinary, 1 a spread split in two and also kept
        // rotated (three output pages), 2 ordinary. A bookmark on source
        // page 2 belongs on the *fifth* output page.
        let page = |role: PageRole, continues_source_page: bool| Page {
            extension: "png".to_string(),
            bytes: tiny_png(),
            role,
            continues_source_page,
            ..Default::default()
        };
        let chapters = vec![Chapter {
            relative_path: PathBuf::from("c001"),
            title: "c001".to_string(),
            pages: vec![
                page(PageRole::Normal, false),
                page(PageRole::SplitFirst, false),
                page(PageRole::SplitSecond, true),
                page(PageRole::Rotated, true),
                page(PageRole::Normal, false),
            ],
        }];
        let mut options = default_options();
        options.bookmarks = vec![(1, "The spread".to_string()), (2, "After it".to_string())];
        let nav = read_entry(build_epub(&chapters, &options).unwrap(), "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"<li><a href="Text/c0001/p0002.xhtml">The spread</a></li>"#));
        assert!(nav.contains(r#"<li><a href="Text/c0001/p0005.xhtml">After it</a></li>"#));
    }

    #[test]
    fn bookmarks_that_name_no_real_page_leave_the_folder_table_of_contents() {
        let mut options = default_options();
        options.bookmarks = vec![(50, "Nowhere".to_string())];
        let nav = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/nav.xhtml",
        );
        assert!(nav.contains("c001 - Title One"));
        assert!(!nav.contains("Nowhere"));
    }

    #[test]
    fn inverted_shifted_and_single_page_spines_match_upstream() {
        // KCC 12.0.0's own `buildOPF()` again, with `--invertdirection`,
        // `--spreadshift` and `--onepagelandscape`:
        // (pages, right_to_left, invert, shift, one_page, sides).
        for (pages, right_to_left, invert, shift, one_page, expected) in [
            (
                "N N N S1 S2 N",
                true,
                true,
                false,
                false,
                "left right left right left right",
            ),
            (
                "N N N S1 S2 N",
                false,
                true,
                false,
                false,
                "right left right left right left",
            ),
            (
                "N N N S1 S2 N",
                true,
                false,
                true,
                false,
                "left right left right left right",
            ),
            (
                "N N N S1 S2 N",
                false,
                false,
                true,
                false,
                "right left right left right left",
            ),
            (
                "N N N S1 S2 N",
                true,
                true,
                true,
                false,
                "left right left right left right",
            ),
            (
                "N N N S1 S2 N",
                true,
                false,
                false,
                true,
                "center center center center center center",
            ),
            (
                "N N R N",
                true,
                true,
                false,
                false,
                "right left center right",
            ),
            (
                "N N R N",
                false,
                true,
                false,
                false,
                "left right center left",
            ),
            (
                "N N R N",
                true,
                false,
                true,
                false,
                "right left center right",
            ),
            (
                "N N R N",
                false,
                false,
                true,
                false,
                "left right center left",
            ),
            (
                "N N R N",
                true,
                true,
                true,
                false,
                "right left center right",
            ),
            (
                "N N R N",
                true,
                false,
                false,
                true,
                "center center center center",
            ),
        ] {
            assert_eq!(
                page_spread_sides(&roles(pages), right_to_left, invert != shift, one_page)
                    .join(" "),
                expected,
                "{pages}, rtl={right_to_left} invert={invert} shift={shift} one_page={one_page}"
            );
        }
    }

    #[test]
    fn invert_direction_flips_the_page_progression_and_writing_mode() {
        let mut options = default_options();
        options.kindle = true;
        options.kindle_resolution = Some((1072, 1448));
        options.invert_direction = true;
        let opf = read_entry(
            build_epub(&sample_chapters(), &options).unwrap(),
            "OEBPS/content.opf",
        );
        // The test book reads right to left; inverted, upstream writes ltr.
        assert!(opf.contains(r#"page-progression-direction="ltr""#));
        assert!(opf.contains(r#"<meta name="primary-writing-mode" content="horizontal-lr"/>"#));
    }
}
