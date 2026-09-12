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
//! Not yet implemented, tracked as gaps rather than guessed at: per-page
//! `page-spread-*` OPF properties (needs [`crate::pipeline::spread`] output
//! tagging that doesn't exist yet) and `ComicInfo.xml`-driven chapter
//! overrides (needs [`crate::metadata`], also not implemented yet).

use super::Chapter;
use crate::error::{Error, Result};
use crate::manga::ReadingDirection;
use image::GenericImageView;
use std::hash::{Hash, Hasher};

pub struct EpubOptions {
    pub title: String,
    pub author: String,
    pub language: String,
    pub reading_direction: ReadingDirection,
}

pub fn build_epub(chapters: &[Chapter], options: &EpubOptions) -> Result<Vec<u8>> {
    let chapters: Vec<&Chapter> = chapters.iter().filter(|c| !c.pages.is_empty()).collect();
    if chapters.is_empty() {
        return Err(Error::EmptyBook);
    }

    let identifier = synthetic_identifier(&options.title);

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
    let mut spine_itemrefs = Vec::new();
    let mut chapter_hrefs = Vec::with_capacity(chapters.len());

    let mut page_counter = 0usize;
    for (chapter_index, chapter) in chapters.iter().enumerate() {
        let mut first_href: Option<String> = None;

        for (page_index, page) in chapter.pages.iter().enumerate() {
            page_counter += 1;
            let (width, height) = image::load_from_memory(&page.bytes)
                .map(|img| img.dimensions())
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
            );
            zip_entries.push((format!("OEBPS/{xhtml_path}"), xhtml.into_bytes()));

            manifest_items.push(format!(
                r#"<item id="img{page_counter}" href="{image_path}" media-type="{media_type}"/>"#
            ));
            manifest_items.push(format!(
                r#"<item id="page{page_counter}" href="{xhtml_path}" media-type="application/xhtml+xml"/>"#
            ));
            spine_itemrefs.push(format!(r#"<itemref idref="page{page_counter}"/>"#));

            first_href.get_or_insert_with(|| xhtml_path.clone());
        }

        chapter_hrefs.push(first_href.expect("chapters with zero pages were filtered out above"));
    }

    zip_entries.push((
        "OEBPS/content.opf".to_string(),
        build_opf(options, &identifier, &manifest_items, &spine_itemrefs).into_bytes(),
    ));
    zip_entries.push((
        "OEBPS/toc.ncx".to_string(),
        build_ncx(options, &identifier, &chapters, &chapter_hrefs).into_bytes(),
    ));
    zip_entries.push((
        "OEBPS/nav.xhtml".to_string(),
        build_nav(options, &chapters, &chapter_hrefs).into_bytes(),
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
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" xml:lang="{lang}">
<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
<dc:identifier id="pub-id">{identifier}</dc:identifier>
<dc:title>{title}</dc:title>
<dc:creator>{author}</dc:creator>
<dc:language>{lang}</dc:language>
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
        author = xml_escape(&options.author),
        items = manifest_items.join("\n"),
        direction = options.reading_direction.epub_page_progression(),
        spine = spine_itemrefs.join("\n"),
    )
}

fn build_page_xhtml(title: &str, image_path_from_oebps: &str, width: u32, height: u32) -> String {
    // Text/cNNNN/pNNNN.xhtml -> Images/cNNNN/pNNNN.ext is always two levels
    // up then back down, given the fixed directory layout above.
    let image_href = format!("../../{image_path_from_oebps}");
    let viewport = if width > 0 && height > 0 {
        format!(r#"<meta name="viewport" content="width={width}, height={height}"/>"#)
    } else {
        String::new()
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<head>
<title>{title}</title>
<meta charset="utf-8"/>
{viewport}
</head>
<body>
<img src="{image_href}" alt=""/>
</body>
</html>"#,
        title = xml_escape(title),
    )
}

fn build_ncx(
    options: &EpubOptions,
    identifier: &str,
    chapters: &[&Chapter],
    chapter_hrefs: &[String],
) -> String {
    let nav_points: String = chapters
        .iter()
        .zip(chapter_hrefs)
        .enumerate()
        .map(|(i, (chapter, href))| {
            format!(
                r#"<navPoint id="chapter{order}" playOrder="{order}"><navLabel><text>{title}</text></navLabel><content src="{href}"/></navPoint>"#,
                order = i + 1,
                title = xml_escape(&chapter.title),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx version="2005-1" xml:lang="{lang}" xmlns="http://www.daisy.org/z3986/2005/ncx/">
<head>
<meta name="dtb:uid" content="{identifier}"/>
<meta name="dtb:depth" content="1"/>
<meta name="dtb:totalPageCount" content="0"/>
<meta name="dtb:maxPageNumber" content="0"/>
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

fn build_nav(options: &EpubOptions, chapters: &[&Chapter], chapter_hrefs: &[String]) -> String {
    let items: String = chapters
        .iter()
        .zip(chapter_hrefs)
        .map(|(chapter, href)| {
            format!(
                r#"<li><a href="{href}">{title}</a></li>"#,
                title = xml_escape(&chapter.title),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

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
</body>
</html>"#,
        title = xml_escape(&options.title),
    )
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

/// Not a real RFC4122 UUID — EPUB only requires `dc:identifier` to be a
/// stable, unique-enough string for this publication, not a cryptographic
/// GUID, so a deterministic hash avoids pulling in a `uuid`/`rand`
/// dependency for this alone.
fn synthetic_identifier(seed: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    format!("urn:mangapress:{:016x}", hasher.finish())
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
                    },
                    Page {
                        extension: "png".to_string(),
                        bytes: tiny_png(),
                    },
                ],
            },
            Chapter {
                relative_path: PathBuf::from("c002 - Nyako's Whereabouts"),
                title: "c002 - Nyako's Whereabouts".to_string(),
                pages: vec![Page {
                    extension: "png".to_string(),
                    bytes: tiny_png(),
                }],
            },
        ]
    }

    fn default_options() -> EpubOptions {
        EpubOptions {
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
            language: "en".to_string(),
            reading_direction: ReadingDirection {
                right_to_left: true,
            },
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

    #[test]
    fn xml_escape_handles_every_special_character() {
        assert_eq!(
            xml_escape("A & B <C> \"D\" 'E'"),
            "A &amp; B &lt;C&gt; &quot;D&quot; &apos;E&apos;"
        );
    }
}
