use super::*;
use clap::Parser;
use image::{DynamicImage, GrayImage, ImageFormat, Luma, Rgb, RgbImage};
use serde_json::Value;
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

fn conversion(input: &Path, flags: &[&str]) -> ResolvedConversion {
    let cli = crate::args::Cli::parse_from(
        [std::ffi::OsStr::new("mangapress"), input.as_os_str()]
            .into_iter()
            .chain(
                [
                    "--profile",
                    "K11",
                    "--format",
                    "cbz",
                    "--customwidth",
                    "32",
                    "--customheight",
                    "48",
                    "--cropping",
                    "disabled",
                    "--noautocontrast",
                    "--forcepng",
                ]
                .iter()
                .map(std::ffi::OsStr::new),
            )
            .chain(flags.iter().map(std::ffi::OsStr::new)),
    );
    crate::configuration::resolve(cli, &mut failure()).unwrap()
}

fn encoded(image: DynamicImage) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn gray(width: u32, height: u32, value: u8) -> Vec<u8> {
    encoded(DynamicImage::ImageLuma8(GrayImage::from_pixel(
        width,
        height,
        Luma([value]),
    )))
}

fn cut_page() -> Vec<u8> {
    let bytes = encoded(DynamicImage::ImageRgb8(RgbImage::from_fn(
        120,
        160,
        |x, y| {
            let noise = (x * 7 + y * 13 + x * y) as u8;
            Rgb([noise, noise.wrapping_mul(3), y as u8])
        },
    )));
    bytes[..bytes.len() * 6 / 10].to_vec()
}

fn strip() -> Vec<u8> {
    encoded(DynamicImage::ImageLuma8(GrayImage::from_fn(
        320,
        1600,
        |x, y| {
            Luma([if (32..288).contains(&x) && (100..1500).contains(&y) {
                if (x / 16) % 2 == 0 {
                    140
                } else {
                    60
                }
            } else {
                255
            }])
        },
    )))
}

fn page(bytes: Vec<u8>, path: Option<&str>) -> Page {
    Page {
        bytes,
        source_path: path.map(PathBuf::from),
        extension: "png".to_string(),
        ..Default::default()
    }
}

fn chapter(path: &str, title: &str, pages: Vec<Page>) -> Chapter {
    Chapter {
        relative_path: PathBuf::from(path),
        title: title.to_string(),
        pages,
    }
}

fn source(chapters: Vec<Chapter>) -> SourceChapters {
    SourceChapters {
        total_chapters: chapters.len(),
        total_pages: chapters.iter().map(|c| c.pages.len()).sum(),
        chapters,
    }
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
fn book_execution_preserves_chapter_identity_and_bytes_with_cumulative_source_progress() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &["--noprocessing"]);
    let originals = [gray(32, 48, 30), gray(32, 48, 90), gray(32, 48, 170)];
    let input = source(vec![
        chapter(
            "",
            "stored root",
            vec![
                page(originals[0].clone(), Some("01.png")),
                page(originals[1].clone(), Some("02.png")),
            ],
        ),
        chapter(
            "Vol.02/Chapter 4",
            "Chapter 4",
            vec![page(originals[2].clone(), Some("Vol.02/Chapter 4/03.png"))],
        ),
    ]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let result =
        process_book(input, &config, "Resolved book", true, &sink, &mut failure()).unwrap();
    assert_eq!(result.total_pages, 3);
    assert_eq!(result.page_count, 3);
    assert_eq!(result.chapters[0].title, "stored root");
    assert_eq!(result.chapters[0].relative_path, PathBuf::new());
    assert_eq!(result.chapters[1].title, "Chapter 4");
    assert_eq!(
        result.chapters[1].relative_path,
        PathBuf::from("Vol.02/Chapter 4")
    );
    for (output, original) in result
        .chapters
        .iter()
        .flat_map(|c| &c.pages)
        .zip(&originals)
    {
        assert_eq!(&output.bytes, original);
        assert_eq!(output.extension, "png");
        assert!(output.source_path.is_some());
    }
    let output = events(&bytes);
    let pages: Vec<_> = output.iter().filter(|e| e["type"] == "page").collect();
    assert_eq!(
        pages
            .iter()
            .map(|e| e["completed"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        pages
            .iter()
            .map(|e| e["page"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert_eq!(pages[0]["chapter"], "Resolved book");
    assert_eq!(pages[2]["chapter"], "Chapter 4");
    assert!(pages.iter().all(|e| e["total"] == 3));
    let completed: Vec<_> = output
        .iter()
        .filter(|e| e["type"] == "chapter" && e["state"] == "completed")
        .collect();
    assert_eq!(completed[0]["completed"], 2);
    assert_eq!(completed[1]["completed"], 3);
    assert_eq!(output.last().unwrap()["source_pages"], 3);
    assert_eq!(output.last().unwrap()["output_pages"], 3);
    assert!(
        !output
            .iter()
            .any(|e| matches!(e["stage"].as_str(), Some("package" | "write"))
                || e["type"] == "result")
    );
}

#[test]
fn split_pages_keep_source_progress_distinct_from_output_count_and_bookmark_continuations() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &[]);
    let input = source(vec![chapter(
        "c001",
        "Wide",
        vec![page(gray(96, 64, 80), Some("wide.png"))],
    )]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let result = process_book(input, &config, "Book", true, &sink, &mut failure()).unwrap();
    assert_eq!(result.total_pages, 1);
    assert_eq!(result.page_count, 2);
    let pages = &result.chapters[0].pages;
    assert_eq!(pages.len(), 2);
    assert!(!pages[0].continues_source_page);
    assert!(pages[1].continues_source_page);
    assert_eq!(pages[0].source_path, pages[1].source_path);
    let output = events(&bytes);
    let progress: Vec<_> = output.iter().filter(|e| e["type"] == "page").collect();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0]["completed"], 1);
    assert_eq!(progress[0]["total"], 1);
    let completed = output
        .iter()
        .find(|e| e["type"] == "chapter" && e["state"] == "completed")
        .unwrap();
    assert_eq!(completed["source_pages"], 1);
    assert_eq!(completed["output_pages"], 2);
}

#[test]
fn webtoon_cutting_updates_counts_before_execution_and_returns_forced_options_and_generated_pages()
{
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(
        directory.path(),
        &[
            "--webtoon",
            "--noprocessing",
            "--manga-style",
            "--upscale",
            "--blackborders",
        ],
    );
    let original = strip();
    let expected = mangapress_core::webtoon::pages_from_chapter(&[&original], (32, 48)).unwrap();
    assert!(expected.len() > 1);
    let input = source(vec![chapter(
        "c001",
        "Strip",
        vec![page(original, Some("original.png"))],
    )]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let result = process_book(input, &config, "Book", true, &sink, &mut failure()).unwrap();
    assert_eq!(result.total_pages, expected.len());
    assert_eq!(result.page_count, expected.len());
    assert_eq!(result.chapters[0].title, "Strip");
    assert!(!result.pipeline_options.manga_style);
    assert!(!result.pipeline_options.upscale);
    assert!(!result.pipeline_options.black_borders);
    assert!(result.pipeline_options.white_borders);
    for (page, original) in result.chapters[0].pages.iter().zip(&expected) {
        assert_eq!(&page.bytes, original);
        assert_eq!(page.extension, "png");
        assert!(page.source_path.is_none());
        assert!(!page.continues_source_page);
    }
    let output = events(&bytes);
    assert_eq!(output[0]["stage"], "process");
    assert_eq!(output[0]["state"], "started");
    assert_eq!(output[0]["pages"], expected.len());
    assert_eq!(output[1]["source_pages"], expected.len());
    assert_eq!(output.last().unwrap()["source_pages"], expected.len());
}

#[test]
fn flat_webtoon_content_retains_empty_chapters_with_zero_progress_counts() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &["--webtoon"]);
    let input = source(vec![chapter(
        "c001",
        "Flat",
        vec![page(gray(320, 800, 90), Some("flat.png"))],
    )]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let result = process_book(input, &config, "Book", true, &sink, &mut failure()).unwrap();
    assert_eq!(result.total_pages, 0);
    assert_eq!(result.page_count, 0);
    assert_eq!(result.chapters.len(), 1);
    assert_eq!(result.chapters[0].title, "Flat");
    assert!(result.chapters[0].pages.is_empty());
    let output = events(&bytes);
    assert_eq!(output.len(), 4);
    assert_eq!(output[0]["pages"], 0);
    assert_eq!(output[1]["source_pages"], 0);
    assert_eq!(output[2]["completed"], 0);
    assert_eq!(output[3]["output_pages"], 0);
    assert!(!output.iter().any(|e| e["type"] == "page"));
}

#[test]
fn webtoon_failure_precedes_process_start_and_keeps_original_chapter_failure_context() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &["--webtoon"]);
    let input = source(vec![chapter(
        "bad",
        "Broken strip",
        vec![page(b"not png".to_vec(), Some("bad.png"))],
    )]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    let error = process_book(input, &config, "Named book", true, &sink, &mut context)
        .err()
        .unwrap();
    assert!(format!("{error:#}").starts_with("cutting chapter 'Broken strip' into pages:"));
    assert_eq!(context.code, "webtoon_split_failed");
    assert_eq!(context.stage, "process");
    assert!(context.recoverable);
    assert_eq!(context.manga.as_deref(), Some("Named book"));
    assert_eq!(context.chapter.as_deref(), Some("Broken strip"));
    assert!(context.path.is_none());
    assert!(context.page.is_none());
    assert!(bytes.is_empty());
}

#[test]
fn later_chapter_failure_keeps_completed_progress_and_uses_input_path_not_image_path() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &[]);
    let input = source(vec![
        chapter(
            "c001",
            "Good",
            vec![page(gray(32, 48, 90), Some("good.png"))],
        ),
        chapter(
            "c002",
            "Bad",
            vec![page(b"bad header".to_vec(), Some("bad/image.png"))],
        ),
    ]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    let error = process_book(input, &config, "Book", true, &sink, &mut context)
        .err()
        .unwrap();
    assert!(format!("{error:#}").starts_with("processing page 1 in chapter 'Bad':"));
    assert!(context.diagnostic.contains("bad/image.png"));
    assert_eq!(context.code, "page_processing_failed");
    assert_eq!(context.stage, "process");
    assert!(context.recoverable);
    assert_eq!(context.manga.as_deref(), Some("Book"));
    assert_eq!(context.chapter.as_deref(), Some("Bad"));
    assert_eq!(context.page, Some(1));
    assert_eq!(context.path.as_deref(), Some(config.input_path.as_str()));
    let output = events(&bytes);
    assert_eq!(output.len(), 5);
    assert_eq!(output[2]["type"], "page");
    assert_eq!(output[2]["completed"], 1);
    assert_eq!(output[3]["type"], "chapter");
    assert_eq!(output[3]["state"], "completed");
    assert_eq!(output[4]["chapter"], "Bad");
    assert_eq!(output[4]["state"], "started");
    assert!(!output
        .iter()
        .any(|e| e["type"] == "stage" && e["state"] == "completed"));
}

#[test]
fn damaged_page_warnings_follow_page_callbacks_before_chapter_completion_with_path_fallback() {
    for known_path in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let config = conversion(directory.path(), &[]);
        let input = source(vec![chapter(
            "c001",
            "Damaged",
            vec![
                page(gray(32, 48, 90), Some("good.png")),
                page(cut_page(), known_path.then_some("c001/cut.png")),
            ],
        )]);
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let result = process_book(input, &config, "Book", true, &sink, &mut failure()).unwrap();
        assert_eq!(result.total_pages, 2);
        assert_eq!(result.page_count, 2);
        let output = events(&bytes);
        assert_eq!(
            output
                .iter()
                .map(|e| e["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["stage", "chapter", "page", "page", "warning", "chapter", "stage"]
        );
        let warning = &output[4];
        assert_eq!(warning["code"], "page_truncated");
        assert_eq!(warning["stage"], "process");
        assert_eq!(warning["page"], 2);
        assert_eq!(warning["chapter"], "Damaged");
        assert_eq!(warning["recoverable"], true);
        assert_eq!(
            warning["path"],
            if known_path {
                "c001/cut.png"
            } else {
                &config.input_path
            }
        );
        assert_eq!(
            warning["message"]
                .as_str()
                .unwrap()
                .contains(" (c001/cut.png)"),
            known_path
        );
        assert!(
            warning["manga"].is_null(),
            "preserve the existing optional-context shape"
        );
    }
}

struct FailAfterEvents {
    remaining: usize,
}
impl std::io::Write for FailAfterEvents {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            Err(std::io::ErrorKind::BrokenPipe.into())
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
fn broken_event_streams_preserve_stage_warning_and_callback_failure_classification() {
    for damaged in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let config = conversion(directory.path(), &[]);
        // Process start, chapter start, page callback, optional warning, chapter end, process end.
        for accepted in 0..if damaged { 6 } else { 5 } {
            let input = source(vec![chapter(
                "c001",
                "Chapter",
                vec![page(
                    if damaged {
                        cut_page()
                    } else {
                        gray(32, 48, 90)
                    },
                    Some("page.png"),
                )],
            )]);
            let sink = EventSink::new(
                true,
                FailAfterEvents {
                    remaining: accepted,
                },
            );
            let mut context = failure();
            let error = process_book(input, &config, "Book", true, &sink, &mut context)
                .err()
                .unwrap();
            if accepted == 2 {
                assert!(error.downcast_ref::<RunFailure>().is_none());
                assert_eq!(context.code, "page_processing_failed");
                assert_eq!(context.page, Some(1));
                assert!(context.diagnostic.contains("writing page progress"));
            } else {
                let protocol = error.downcast_ref::<RunFailure>().unwrap();
                assert_eq!(protocol.code, "event_write_failed");
                assert_eq!(protocol.stage, "protocol");
                assert!(!protocol.recoverable);
                assert_eq!(context.code, "conversion_failed");
            }
        }
    }
}

#[test]
fn first_page_flag_only_applies_to_the_first_chapter_and_keeps_resolved_options() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = conversion(directory.path(), &["--forcecolor"]);
    config.cli.cropping = crate::args::Cropping::Margins;
    let image = encoded(DynamicImage::ImageRgb8(RgbImage::from_fn(
        32,
        48,
        |x, y| {
            if (8..24).contains(&x) && (8..40).contains(&y) {
                Rgb([180, 30, 60])
            } else {
                Rgb([255, 255, 255])
            }
        },
    )));
    let original = page(image, Some("color.png"));
    let options =
        crate::configuration::pipeline_options(&config.cli, config.profile, config.output_format);
    let first = process_chapter_pages(std::slice::from_ref(&original), &options, true, |_, _| {
        Ok(())
    })
    .unwrap();
    let second = process_chapter_pages(std::slice::from_ref(&original), &options, false, |_, _| {
        Ok(())
    })
    .unwrap();
    assert_ne!(
        first.pages[0].bytes, second.pages[0].bytes,
        "fixture distinguishes cover handling"
    );
    let input = source(vec![
        chapter("c001", "First", vec![original.clone()]),
        chapter("c002", "Second", vec![original]),
    ]);
    let sink = EventSink::new(false, std::io::sink());
    let result = process_book(input, &config, "Book", true, &sink, &mut failure()).unwrap();
    assert_eq!(result.chapters[0].pages[0].bytes, first.pages[0].bytes);
    assert_eq!(result.chapters[1].pages[0].bytes, second.pages[0].bytes);
    assert_eq!(result.pipeline_options.target_resolution(), (32, 48));
    assert!(result.pipeline_options.force_color);
    assert!(matches!(
        result.pipeline_options.cropping,
        mangapress_core::pipeline::CroppingMode::Margins
    ));
}
