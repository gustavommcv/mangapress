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

fn loose_page() -> Page {
    Page {
        extension: "png".to_string(),
        bytes: tiny_png(),
        ..Default::default()
    }
}

fn chapter_in(folder: &str, title: &str) -> Chapter {
    Chapter {
        relative_path: PathBuf::from(folder),
        title: title.to_string(),
        pages: vec![loose_page()],
    }
}

fn navigation_of(bytes: Vec<u8>) -> (String, String) {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut read = |name: &str| {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(name).unwrap(), &mut text).unwrap();
        text
    };
    (read("OEBPS/toc.ncx"), read("OEBPS/nav.xhtml"))
}

#[test]
fn pages_lying_directly_in_the_book_are_listed_under_the_books_title() {
    // `group_into_chapters` calls the folderless group "Untitled"; the contents say the title.
    let chapters = vec![chapter_in("", "Untitled")];

    let (ncx, nav) = navigation_of(build_epub(&chapters, &default_options()).unwrap());

    assert_eq!(ncx.matches("<navPoint").count(), 1);
    assert!(ncx.contains("<navLabel><text>Test Book</text></navLabel>"));
    assert!(nav.contains(">Test Book</a></li>"));
    assert!(!ncx.contains("Untitled") && !nav.contains("Untitled"));
}

#[test]
fn a_folder_beside_loose_pages_keeps_its_own_name() {
    let chapters = vec![
        chapter_in("", "Untitled"),
        chapter_in("c001 - One", "c001 - One"),
    ];

    let (ncx, nav) = navigation_of(build_epub(&chapters, &default_options()).unwrap());

    let book = ncx
        .find("<navLabel><text>Test Book</text></navLabel>")
        .unwrap();
    let folder = ncx
        .find("<navLabel><text>c001 - One</text></navLabel>")
        .unwrap();
    assert!(book < folder);
    assert!(nav.contains(">c001 - One</a></li>"));
}

#[test]
fn a_bookmark_keeps_the_name_it_was_given() {
    let chapters = vec![chapter_in("", "Untitled")];
    let mut options = default_options();
    options.bookmarks = vec![(0, "The start".to_string())];

    let (ncx, nav) = navigation_of(build_epub(&chapters, &options).unwrap());

    assert!(ncx.contains("<navLabel><text>The start</text></navLabel>"));
    assert!(nav.contains(">The start</a></li>"));
    assert!(!ncx.contains("<navLabel><text>Test Book</text></navLabel>"));
}

#[test]
fn the_identifier_keeps_being_made_from_the_groups_own_title() {
    // Only the label written into the contents changes; a book keeps the identifier it had.
    let chapters = vec![chapter_in("", "Untitled")];
    let bytes = build_epub(&chapters, &default_options()).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut opf = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("OEBPS/content.opf").unwrap(), &mut opf)
        .unwrap();

    let expected = synthetic_identifier("Test Book\u{0}Test Author\u{0}Untitled");

    assert!(opf.contains(&expected), "{opf}");
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
    // feature is for, not just "every label shows up somewhere". IDs are
    // unique across nodes; playOrder instead counts distinct targets.
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
fn ncx_play_order_counts_targets_not_nodes_and_depth_matches_the_tree() {
    let chapters = nested_sample_chapters();
    for (nested, bookmarks, expected_orders, expected_depth) in [
        (true, vec![], vec![1, 1, 2, 3, 3], "2"),
        (false, vec![], vec![1, 2, 3], "1"),
        (
            true,
            vec![(0, "Opening"), (0, "Alias"), (2, "Later")],
            vec![1, 1, 2],
            "1",
        ),
    ] {
        let mut options = default_options();
        options.nested_toc = nested;
        options.bookmarks = bookmarks
            .into_iter()
            .map(|(index, label)| (index, label.to_string()))
            .collect();
        let ncx = read_entry(build_epub(&chapters, &options).unwrap(), "OEBPS/toc.ncx");
        let document = roxmltree::Document::parse(&ncx).unwrap();
        let nodes: Vec<_> = document
            .descendants()
            .filter(|node| node.has_tag_name("navPoint"))
            .collect();
        let ids: std::collections::HashSet<_> = nodes
            .iter()
            .map(|node| node.attribute("id").unwrap())
            .collect();
        assert_eq!(ids.len(), nodes.len(), "every navigation ID must be unique");
        let orders: Vec<usize> = nodes
            .iter()
            .map(|node| node.attribute("playOrder").unwrap().parse().unwrap())
            .collect();
        assert_eq!(orders, expected_orders);
        let mut target_orders = std::collections::HashMap::new();
        for (node, order) in nodes.iter().zip(orders) {
            let target = node
                .children()
                .find(|child| child.has_tag_name("content"))
                .unwrap()
                .attribute("src")
                .unwrap();
            if let Some(previous) = target_orders.insert(target, order) {
                assert_eq!(order, previous, "the same target must keep its playOrder");
            }
        }
        let depth = document
            .descendants()
            .find(|node| node.attribute("name") == Some("dtb:depth"))
            .unwrap()
            .attribute("content");
        assert_eq!(depth, Some(expected_depth));
    }
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
    assert!(opf.contains(r#"<itemref idref="page1" properties="rendition:page-spread-right"/>"#));
    assert!(opf.contains(r#"<itemref idref="page2" properties="rendition:page-spread-left"/>"#));

    let mut kindle = default_options();
    kindle.kindle = true;
    let opf = read_entry(
        build_epub(&sample_chapters(), &kindle).unwrap(),
        "OEBPS/content.opf",
    );
    assert!(opf.contains(r#"<itemref idref="page1" linear="yes" properties="page-spread-right"/>"#));
}

#[test]
fn centered_spine_items_use_the_reserved_rendition_property_for_every_family() {
    for kindle in [false, true] {
        for right_to_left in [false, true] {
            for one_page_landscape in [false, true] {
                let mut chapters = sample_chapters();
                chapters[0].pages[1].role = PageRole::Rotated;
                let mut options = default_options();
                options.kindle = kindle;
                options.reading_direction.right_to_left = right_to_left;
                options.one_page_landscape = one_page_landscape;
                let opf = read_entry(
                    build_epub(&chapters, &options).unwrap(),
                    "OEBPS/content.opf",
                );
                let document = roxmltree::Document::parse(&opf).unwrap();
                let items: Vec<_> = document
                    .descendants()
                    .filter(|node| node.has_tag_name("itemref"))
                    .collect();
                let sides = if one_page_landscape {
                    ["center", "center", "center"]
                } else if right_to_left {
                    ["left", "center", "right"]
                } else {
                    ["right", "center", "left"]
                };
                assert_eq!(items.len(), sides.len());
                for (index, (item, side)) in items.iter().zip(sides).enumerate() {
                    let prefix = if kindle && side != "center" {
                        ""
                    } else {
                        "rendition:"
                    };
                    assert_eq!(
                        item.attribute("properties"),
                        Some(format!("{prefix}page-spread-{side}").as_str()),
                        "kindle={kindle}, rtl={right_to_left}, one_page={one_page_landscape}"
                    );
                    assert_eq!(
                        item.attribute("idref"),
                        Some(format!("page{}", index + 1).as_str())
                    );
                    assert_eq!(item.attribute("linear"), kindle.then_some("yes"));
                }
            }
        }
    }
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
    assert!(page_list.contains(r#"<li><a href="Text/c0001/p0001.xhtml">c001 - Title One</a></li>"#));
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
        0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30,
        0xc8,
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
    assert!(
        opf.contains(r#"<item id="img1" href="Images/c0001/p0001.png" media-type="image/png"/>"#)
    );
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
    assert!(toc.contains(r#"<li><a href="Text/c0002/p0001.xhtml">Second half &amp; more</a></li>"#));
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
            page_spread_sides(&roles(pages), right_to_left, invert != shift, one_page).join(" "),
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
