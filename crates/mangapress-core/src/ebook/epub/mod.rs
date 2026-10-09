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
#[cfg(test)]
use crate::pipeline::spread::PageRole;

mod identifier;
mod navigation;
mod package;
mod page;
mod spine;

use identifier::synthetic_identifier;
use navigation::{bookmark_toc, build_nav, build_ncx};
use package::{build_container_xml, build_opf};
use page::build_page_xhtml;
use spine::page_spread_sides;

#[cfg(test)]
use identifier::{sha1, uuid_v5, MANGAPRESS_NAMESPACE, URL_NAMESPACE};

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

#[cfg(test)]
mod tests;
