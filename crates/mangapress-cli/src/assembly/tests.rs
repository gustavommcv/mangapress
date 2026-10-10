use super::*;
use clap::Parser;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage};
use mangapress_core::ebook::{Chapter, Page};
use mangapress_core::metadata::{ComicInfo, ResolvedMetadata};
use serde_json::Value;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

fn failure() -> RunFailure {
    RunFailure::new(
        "conversion_failed",
        "conversion",
        false,
        "initial",
        "initial",
    )
}

fn conversion(input: &Path, profile: &str, format: &str, flags: &[&str]) -> ResolvedConversion {
    let cli = crate::args::Cli::parse_from(
        [std::ffi::OsStr::new("mangapress"), input.as_os_str()]
            .into_iter()
            .chain(
                ["--profile", profile, "--format", format]
                    .iter()
                    .map(std::ffi::OsStr::new),
            )
            .chain(flags.iter().map(std::ffi::OsStr::new)),
    );
    crate::configuration::resolve(cli, &mut failure()).unwrap()
}

fn encoded(image: DynamicImage) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn colorful_cover() -> Vec<u8> {
    encoded(DynamicImage::ImageRgb8(RgbImage::from_fn(
        96,
        64,
        |x, y| Rgb([(x * 2) as u8, (y * 3) as u8, (x + y) as u8]),
    )))
}

fn metadata() -> InputMetadata {
    InputMetadata {
        resolved: ResolvedMetadata {
            title: "Book & title".to_string(),
            authors: vec!["Writer".into(), "Artist".into()],
            summary: Some("A <summary>".into()),
            series: Some("Series & name".into()),
            series_position: Some("2.5".into()),
        },
        author: "Combined authors".into(),
        comic_info: None,
        comic_info_xml: Some(
            b"<?xml version=\"1.0\"?>\r\n<ComicInfo><Title>Original</Title></ComicInfo>\r\n"
                .to_vec(),
        ),
    }
}

fn processed(config: &ResolvedConversion) -> ProcessedBook {
    let chapters = ["Vol.01/Chapter 1", "Vol.02/Chapter 2"]
        .into_iter()
        .map(|path| Chapter {
            relative_path: PathBuf::from(path),
            title: path.rsplit('/').next().unwrap().into(),
            pages: vec![Page {
                bytes: encoded(DynamicImage::ImageLuma8(GrayImage::from_pixel(
                    32,
                    48,
                    Luma([90]),
                ))),
                extension: "png".into(),
                ..Default::default()
            }],
        })
        .collect();
    ProcessedBook {
        chapters,
        total_pages: 2,
        page_count: 2,
        pipeline_options: crate::configuration::pipeline_options(
            &config.cli,
            config.profile,
            config.output_format,
        ),
    }
}

fn planned(directory: &Path, format: &'static str) -> PlannedOutput {
    let output_path = directory.join(format!("book.{format}"));
    PlannedOutput {
        output_path_absolute: output_path.to_string_lossy().into_owned(),
        output_path,
        format,
        extension: format,
        staged_output: None,
    }
}

fn assets(joined: &spreads::Joined) -> BookAssets<'_> {
    BookAssets {
        cover_source: None,
        custom_cover: false,
        joined_spreads: joined,
    }
}

fn member(bytes: &[u8], path: &str) -> Vec<u8> {
    let mut book = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    book.by_name(path)
        .unwrap()
        .read_to_end(&mut result)
        .unwrap();
    result
}

fn xml(text: &str) -> roxmltree::Document<'_> {
    roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn events(bytes: &[u8]) -> Vec<Value> {
    std::str::from_utf8(bytes)
        .unwrap()
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let event: Value = serde_json::from_str(line).unwrap();
            assert_eq!(event["sequence"], index + 1);
            assert_eq!(event["protocol_version"], 1);
            event
        })
        .collect()
}

#[test]
fn assembled_epub_keeps_metadata_nested_navigation_and_package_events() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(
        directory.path(),
        "KoLC",
        "epub",
        &["--language", "pt", "--nested-toc", "--manga-style"],
    );
    let pages = processed(&config);
    let output = planned(directory.path(), "epub");
    let joined = spreads::Joined::default();
    let mut log = Vec::new();
    let book = assemble(
        metadata(),
        &pages,
        assets(&joined),
        &config,
        &output,
        &EventSink::new(true, &mut log),
        &mut failure(),
    )
    .unwrap();
    assert_eq!(book.title, "Book & title");
    assert_eq!(book.author, "Combined authors");
    let package = String::from_utf8(member(&book.bytes, "OEBPS/content.opf")).unwrap();
    let opf = xml(&package);
    for (tag, value) in [
        ("title", "Book & title"),
        ("language", "pt"),
        ("description", "A <summary>"),
    ] {
        assert_eq!(
            opf.descendants()
                .find(|n| n.has_tag_name(tag))
                .unwrap()
                .text(),
            Some(value)
        );
    }
    assert_eq!(
        opf.descendants()
            .filter(|n| n.has_tag_name("creator"))
            .map(|n| n.text().unwrap())
            .collect::<Vec<_>>(),
        ["Writer", "Artist"]
    );
    for (property, value) in [
        ("belongs-to-collection", "Series & name"),
        ("group-position", "2.5"),
    ] {
        assert_eq!(
            opf.descendants()
                .find(|n| n.attribute("property") == Some(property))
                .unwrap()
                .text(),
            Some(value)
        );
    }
    assert_eq!(
        opf.descendants()
            .find(|n| n.has_tag_name("spine"))
            .unwrap()
            .attribute("page-progression-direction"),
        Some("rtl")
    );
    let navigation = String::from_utf8(member(&book.bytes, "OEBPS/nav.xhtml")).unwrap();
    let nav = xml(&navigation);
    let toc = nav
        .descendants()
        .find(|n| n.attribute("id") == Some("toc"))
        .unwrap();
    let outer = toc.children().find(|n| n.has_tag_name("ol")).unwrap();
    let volumes = outer
        .children()
        .filter(|n| n.has_tag_name("li"))
        .collect::<Vec<_>>();
    assert_eq!(volumes.len(), 2);
    for (volume, (name, chapter)) in volumes
        .iter()
        .zip([("Vol.01", "Chapter 1"), ("Vol.02", "Chapter 2")])
    {
        assert_eq!(
            volume
                .children()
                .find(|n| n.has_tag_name("a"))
                .unwrap()
                .text(),
            Some(name)
        );
        let nested = volume.children().find(|n| n.has_tag_name("ol")).unwrap();
        assert_eq!(
            nested
                .descendants()
                .find(|n| n.has_tag_name("a"))
                .unwrap()
                .text(),
            Some(chapter)
        );
    }
    let log = events(&log);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0]["stage"], "package");
    assert_eq!(log[0]["state"], "started");
    assert_eq!(log[1]["stage"], "package");
    assert_eq!(log[1]["state"], "completed");
    assert_eq!(log[1]["bytes"], book.bytes.len());
    assert!(
        !output.output_path.exists(),
        "assembly does not publish files"
    );
}

#[test]
fn epub_device_handoff_keeps_native_scribe_cap_custom_overrides_and_family_markup() {
    let directory = tempfile::tempdir().unwrap();
    for (profile, flags, resolution, kindle) in [
        ("K11", vec![], Some("1072x1448"), true),
        ("KS3", vec![], Some("1920x2648"), true),
        ("K11", vec!["--customwidth", "1072"], None, true),
        ("K11", vec!["--customheight", "1448"], None, true),
        ("KoLC", vec![], None, false),
    ] {
        let config = conversion(directory.path(), profile, "epub", &flags);
        let pages = processed(&config);
        let output = planned(directory.path(), "epub");
        let joined = spreads::Joined::default();
        let book = assemble(
            metadata(),
            &pages,
            assets(&joined),
            &config,
            &output,
            &EventSink::new(false, std::io::sink()),
            &mut failure(),
        )
        .unwrap();
        let package = String::from_utf8(member(&book.bytes, "OEBPS/content.opf")).unwrap();
        let opf = xml(&package);
        let original = opf
            .descendants()
            .find(|n| n.attribute("name") == Some("original-resolution"))
            .and_then(|n| n.attribute("content"));
        assert_eq!(original, resolution, "{profile} {flags:?}");
        assert_eq!(
            opf.descendants()
                .any(|n| n.attribute("property") == Some("belongs-to-collection")),
            !kindle
        );
        let page = String::from_utf8(member(&book.bytes, "OEBPS/Text/c0001/p0001.xhtml")).unwrap();
        assert_eq!(
            page.contains("<div style=\"display:none;\">.</div>"),
            kindle
        );
    }
}

#[test]
fn epub_joined_spread_bookmarks_keep_targets_and_continuation_pages() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "epub", &[]);
    let mut pages = processed(&config);
    let original = pages.chapters[0].pages[0].clone();
    pages.chapters.truncate(1);
    pages.chapters[0].pages = vec![original; 4];
    pages.chapters[0].pages[1].continues_source_page = true;
    pages.total_pages = 3;
    pages.page_count = 4;
    let mut info = metadata();
    info.comic_info = Some(ComicInfo {
        bookmarks: (0..4).map(|i| (i, format!("Bookmark {i}"))).collect(),
        ..Default::default()
    });
    let joined = spreads::Joined {
        joined: vec![0],
        ..Default::default()
    };
    let book = assemble(
        info,
        &pages,
        assets(&joined),
        &config,
        &planned(directory.path(), "epub"),
        &EventSink::new(false, std::io::sink()),
        &mut failure(),
    )
    .unwrap();
    let navigation = String::from_utf8(member(&book.bytes, "OEBPS/nav.xhtml")).unwrap();
    let nav = xml(&navigation);
    let toc = nav
        .descendants()
        .find(|n| n.attribute("id") == Some("toc"))
        .unwrap();
    let links = toc
        .descendants()
        .filter(|n| n.has_tag_name("a"))
        .collect::<Vec<_>>();
    assert_eq!(
        links
            .iter()
            .map(|n| n.attribute("href").unwrap())
            .collect::<Vec<_>>(),
        [
            "Text/c0001/p0001.xhtml",
            "Text/c0001/p0001.xhtml",
            "Text/c0001/p0003.xhtml",
            "Text/c0001/p0004.xhtml"
        ]
    );
    assert_eq!(
        links.iter().map(|n| n.text().unwrap()).collect::<Vec<_>>(),
        ["Bookmark 0", "Bookmark 1", "Bookmark 2", "Bookmark 3"]
    );
}

#[test]
fn cover_handoff_preserves_profile_quality_custom_target_color_and_webtoon_direction() {
    let directory = tempfile::tempdir().unwrap();
    let image = colorful_cover();
    let joined = spreads::Joined::default();
    for (profile, flags, expected) in [
        (
            "K11",
            vec!["--manga-style", "--smartcovercrop", "--forcecolor"],
            cover::CoverOptions {
                target: (1072, 1448),
                right_to_left: true,
                smart_crop: true,
                fill: false,
                force_color: true,
                jpeg_quality: 85,
            },
        ),
        (
            "KS3",
            vec![],
            cover::CoverOptions {
                target: (1920, 2648),
                right_to_left: false,
                smart_crop: false,
                fill: false,
                force_color: false,
                jpeg_quality: 90,
            },
        ),
        (
            "KCS",
            vec!["--customwidth", "32", "--customheight", "48"],
            cover::CoverOptions {
                target: (32, 48),
                right_to_left: false,
                smart_crop: false,
                fill: false,
                force_color: false,
                jpeg_quality: 85,
            },
        ),
        (
            "KCS",
            vec![
                "--customwidth",
                "32",
                "--customheight",
                "48",
                "--webtoon",
                "--manga-style",
                "--smartcovercrop",
                "--coverfill",
                "--forcecolor",
                "--jpeg-quality",
                "73",
            ],
            cover::CoverOptions {
                target: (32, 48),
                right_to_left: false,
                smart_crop: true,
                fill: true,
                force_color: true,
                jpeg_quality: 73,
            },
        ),
    ] {
        let config = conversion(directory.path(), profile, "epub", &flags);
        let pages = processed(&config);
        let input = BookAssets {
            cover_source: Some(&image),
            ..assets(&joined)
        };
        let actual = build_cover(
            &input,
            &config,
            &pages.pipeline_options,
            "Book",
            &mut failure(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            actual,
            cover::build_cover_reporting(&image, &expected).unwrap(),
            "{profile} {flags:?}"
        );
    }
}

#[test]
fn cbz_cover_and_xml_rules_keep_original_payloads_and_extra_entry_order() {
    let directory = tempfile::tempdir().unwrap();
    let image = colorful_cover();
    let joined = spreads::Joined::default();
    for custom in [false, true] {
        for smart in [false, true] {
            for keep in [false, true] {
                let mut flags = vec![];
                if smart {
                    flags.push("--smartcovercrop");
                }
                if keep {
                    flags.push("--keepcomicinfo");
                }
                let config = conversion(directory.path(), "K11", "cbz", &flags);
                let pages = processed(&config);
                let info = metadata();
                let original_xml = info.comic_info_xml.clone().unwrap();
                let book = assemble(
                    info,
                    &pages,
                    BookAssets {
                        cover_source: Some(&image),
                        custom_cover: custom,
                        joined_spreads: &joined,
                    },
                    &config,
                    &planned(directory.path(), "cbz"),
                    &EventSink::new(false, std::io::sink()),
                    &mut failure(),
                )
                .unwrap();
                let mut archive = zip::ZipArchive::new(Cursor::new(&book.bytes)).unwrap();
                let names = (0..archive.len())
                    .map(|i| archive.by_index(i).unwrap().name().to_owned())
                    .collect::<Vec<_>>();
                assert_eq!(names.iter().any(|n| n == "##cover.jpg"), custom || smart);
                assert_eq!(names.iter().any(|n| n == "ComicInfo.xml"), keep);
                if custom || smart {
                    assert_eq!(names[0], "##cover.jpg");
                }
                if keep {
                    assert_eq!(names[usize::from(custom || smart)], "ComicInfo.xml");
                    assert_eq!(member(&book.bytes, "ComicInfo.xml"), original_xml);
                }
                let encoded_pages = names
                    .iter()
                    .filter(|n| n.ends_with(".png"))
                    .map(|n| member(&book.bytes, n))
                    .collect::<Vec<_>>();
                assert_eq!(
                    encoded_pages,
                    pages
                        .chapters
                        .iter()
                        .map(|c| c.pages[0].bytes.clone())
                        .collect::<Vec<_>>()
                );
                assert_eq!(book.author, "Combined authors");
            }
        }
    }
}

#[test]
fn pdf_ignores_unreadable_cover_and_uses_the_original_title_and_author() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "Rmk2", "pdf", &[]);
    let pages = processed(&config);
    let output = planned(directory.path(), "pdf");
    let joined = spreads::Joined::default();
    let mut log = Vec::new();
    let book = assemble(
        metadata(),
        &pages,
        BookAssets {
            cover_source: Some(b"broken image"),
            custom_cover: true,
            joined_spreads: &joined,
        },
        &config,
        &output,
        &EventSink::new(true, &mut log),
        &mut failure(),
    )
    .unwrap();
    assert!(book.bytes.starts_with(b"%PDF"));
    let document = String::from_utf8_lossy(&book.bytes);
    // Separate builder calls advance its generated identifiers. Check the
    // handoff here; fresh-process release contracts compare complete PDF bytes.
    for (key, value) in [("Title", "Book & title"), ("Author", "Combined authors")] {
        let mut encoded = "FEFF".to_string();
        for unit in value.encode_utf16() {
            encoded.push_str(&format!("{unit:04X}"));
        }
        assert!(document.contains(&format!("/{key}<{encoded}>")));
    }
    assert!(document.contains("/Count 2"));
    assert_eq!(book.author, "Combined authors");
    assert_eq!(events(&log).len(), 2);
    assert!(!output.output_path.exists());
}

#[test]
fn empty_books_keep_package_failure_context_and_emit_no_completed_event() {
    let directory = tempfile::tempdir().unwrap();
    for format in ["epub", "cbz", "pdf"] {
        let config = conversion(directory.path(), "K11", format, &[]);
        let mut pages = processed(&config);
        pages.chapters.clear();
        pages.page_count = 0;
        let output = planned(directory.path(), format);
        let joined = spreads::Joined::default();
        let mut context = failure();
        let mut log = Vec::new();
        assert!(assemble(
            metadata(),
            &pages,
            assets(&joined),
            &config,
            &output,
            &EventSink::new(true, &mut log),
            &mut context
        )
        .is_err());
        assert_eq!(context.code, "book_build_failed");
        assert_eq!(context.stage, "package");
        assert!(!context.recoverable);
        assert_eq!(context.manga.as_deref(), Some("Book & title"));
        assert_eq!(
            context.path.as_deref(),
            Some(output.output_path_absolute.as_str())
        );
        let events = events(&log);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["state"], "started");
        assert!(!output.output_path.exists());
    }
}

#[test]
fn cover_errors_replace_book_context_before_any_completed_package_or_write() {
    let directory = tempfile::tempdir().unwrap();
    for format in ["epub", "cbz"] {
        let config = conversion(directory.path(), "K11", format, &[]);
        let pages = processed(&config);
        let output = planned(directory.path(), format);
        let joined = spreads::Joined::default();
        let mut context = failure();
        let mut log = Vec::new();
        let error = assemble(
            metadata(),
            &pages,
            BookAssets {
                cover_source: Some(b"broken image"),
                ..assets(&joined)
            },
            &config,
            &output,
            &EventSink::new(true, &mut log),
            &mut context,
        )
        .err()
        .unwrap();
        assert!(error.to_string().starts_with("building the cover:"));
        assert_eq!(context.code, "cover_build_failed");
        assert_eq!(context.stage, "package");
        assert!(context.recoverable);
        assert_eq!(context.manga.as_deref(), Some("Book & title"));
        assert!(context.path.is_none());
        assert_eq!(events(&log).len(), 1);
        assert!(!output.output_path.exists());
    }
}

struct FailAfterEvents {
    remaining: usize,
}

impl std::io::Write for FailAfterEvents {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            Err(std::io::Error::other("assembly test writer stopped"))
        } else {
            Ok(bytes.len())
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.remaining -= 1;
        Ok(())
    }
}

#[test]
fn package_event_write_failures_keep_protocol_classification_and_context_order() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "cbz", &[]);
    let pages = processed(&config);
    let output = planned(directory.path(), "cbz");
    let joined = spreads::Joined::default();
    for accepted in 0..2 {
        let mut context = failure();
        let sink = EventSink::new(
            true,
            FailAfterEvents {
                remaining: accepted,
            },
        );
        let error = assemble(
            metadata(),
            &pages,
            assets(&joined),
            &config,
            &output,
            &sink,
            &mut context,
        )
        .err()
        .unwrap();
        let protocol = error.downcast_ref::<RunFailure>().unwrap();
        assert_eq!(protocol.code, "event_write_failed");
        assert_eq!(protocol.stage, "protocol");
        assert!(!protocol.recoverable);
        assert_eq!(
            context.code,
            if accepted == 0 {
                "conversion_failed"
            } else {
                "book_build_failed"
            }
        );
        assert_eq!(context.path.is_some(), accepted == 1);
        assert!(!output.output_path.exists());
    }
}
