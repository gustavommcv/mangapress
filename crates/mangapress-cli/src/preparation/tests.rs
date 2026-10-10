use super::*;
use clap::Parser;
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;

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
            .chain(flags.iter().map(std::ffi::OsStr::new)),
    );
    crate::configuration::resolve(cli, &mut failure()).unwrap()
}

fn png(width: u32, height: u32, gray: u8) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(GrayImage::from_pixel(width, height, Luma([gray])))
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn entry(path: &str, bytes: Vec<u8>) -> SourceEntry {
    SourceEntry {
        relative_path: path.into(),
        bytes,
    }
}

fn input(entries: Vec<SourceEntry>) -> BookInput {
    BookInput {
        entries,
        ..Default::default()
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

fn warnings(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .filter(|event| event["type"] == "warning")
        .map(|event| event["code"].as_str().unwrap())
        .collect()
}

#[test]
fn preparation_retains_raw_pages_paths_and_order_while_aggregating_non_image_skips() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), &["--upscale"]);
    let mut source = input(vec![
        entry("Second/002.PNG", b"second untouched page".to_vec()),
        entry("First/001.png", b"first untouched page".to_vec()),
        entry("Second/003.png", b"third untouched page".to_vec()),
        entry("notes.txt", b"notes".to_vec()),
        entry("__MACOSX/ignored.png", b"sidecar".to_vec()),
        entry("._ignored.png", b"sidecar".to_vec()),
    ]);
    source.skipped_non_images = 2;
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    let prepared = prepare(source, &config, "Resolved title", true, &sink, &mut context).unwrap();
    assert_eq!((prepared.total_chapters, prepared.total_pages), (2, 3));
    let chapters = &prepared.source_chapters;
    assert_eq!(chapters[0].relative_path, Path::new("Second"));
    assert_eq!(chapters[1].relative_path, Path::new("First"));
    assert_eq!(
        chapters[0].pages[0].source_path.as_deref(),
        Some(Path::new("Second/002.PNG"))
    );
    assert_eq!(chapters[0].pages[0].extension, "png");
    assert_eq!(chapters[0].pages[0].bytes, b"second untouched page");
    assert_eq!(chapters[0].pages[1].bytes, b"third untouched page");
    assert_eq!(chapters[1].pages[0].bytes, b"first untouched page");
    assert_eq!(
        prepared.cover_source.as_deref(),
        Some(b"second untouched page".as_slice())
    );
    assert!(prepared.custom_cover.is_none());
    assert!(prepared.joined_spreads.joined.is_empty());
    assert_eq!(context.code, "conversion_failed");
    let output = events(&bytes);
    assert_eq!(warnings(&output), ["skipped_non_images"]);
    assert_eq!(output[0]["count"], 5);
    assert_eq!(output[0]["path"], config.input_path);
    assert_eq!(output[1]["stage"], "inspect");
    assert_eq!(output[1]["state"], "completed");
    assert_eq!(output[1]["manga"], "Resolved title");
    assert_eq!(output[1]["chapters"], 2);
    assert_eq!(output[1]["pages"], 3);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn no_pages_warns_and_fails_before_cover_or_spread_resolution() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = conversion(directory.path(), &[]);
    config.cli.cover = Some(directory.path().join("missing-cover.png"));
    config.cli.spreads = Some(directory.path().join("missing-labels.json"));
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    let error = prepare(
        input(vec![entry("notes.txt", vec![])]),
        &config,
        "Title",
        true,
        &sink,
        &mut context,
    )
    .err()
    .unwrap();
    assert_eq!(context.code, "no_page_images");
    assert_eq!(context.stage, "inspect");
    assert!(context.recoverable);
    assert_eq!(context.path.as_deref(), Some(config.input_path.as_str()));
    assert_eq!(error.to_string(), context.diagnostic);
    let output = events(&bytes);
    assert_eq!(output.len(), 1);
    assert_eq!(warnings(&output), ["skipped_non_images"]);
    assert_eq!(output[0]["count"], 1);
}

#[test]
fn size_warning_uses_strict_quarter_threshold_measured_pages_and_existing_exclusions() {
    let directory = tempfile::tempdir().unwrap();
    let base = [
        "--profile",
        "K11",
        "--customwidth",
        "120",
        "--customheight",
        "180",
    ];
    for (small, extra, expected) in [
        (1, vec![], false),
        (2, vec![], true),
        (2, vec!["--upscale"], false),
        (2, vec!["--stretch"], false),
        (2, vec!["--webtoon"], false),
    ] {
        let flags: Vec<_> = base.into_iter().chain(extra).collect();
        let config = conversion(directory.path(), &flags);
        let mut entries: Vec<_> = (0..4)
            .map(|index| {
                let (width, height) = if index < small { (32, 48) } else { (120, 48) };
                entry(&format!("{index}-kcc.png"), png(width, height, 80))
            })
            .collect();
        entries.push(entry(
            "unmeasurable.png",
            b"not a decodable header".to_vec(),
        ));
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let prepared = prepare(
            input(entries),
            &config,
            "Title",
            true,
            &sink,
            &mut failure(),
        )
        .unwrap();
        assert_eq!(prepared.total_pages, 5);
        let output = events(&bytes);
        assert_eq!(
            warnings(&output),
            if expected {
                vec!["source_already_converted", "images_smaller_than_device"]
            } else {
                vec!["source_already_converted"]
            }
        );
        if expected {
            assert_eq!(output[1]["message"], "2 of 4 pages are smaller than the device's 120x180 screen. Consider --upscale (or --stretch) to make them easier to read.");
        }
    }
    let config = conversion(
        directory.path(),
        &[
            "--profile",
            "KS3",
            "--customwidth",
            "120",
            "--customheight",
            "180",
        ],
    );
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    prepare(
        input(vec![entry("001.png", png(32, 48, 80))]),
        &config,
        "Title",
        true,
        &sink,
        &mut failure(),
    )
    .unwrap();
    assert!(warnings(&events(&bytes)).is_empty());
}

#[test]
fn explicit_cover_wins_convention_and_both_preserve_the_original_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let book = directory.path().join("Source");
    std::fs::create_dir(&book).unwrap();
    std::fs::create_dir(directory.path().join("Covers")).unwrap();
    std::fs::write(
        directory.path().join("Covers/Source.png"),
        b"convention cover",
    )
    .unwrap();
    let explicit = directory.path().join("explicit.png");
    std::fs::write(&explicit, b"explicit cover").unwrap();
    let sink = EventSink::new(false, std::io::sink());
    for (named, expected) in [
        (false, b"convention cover".as_slice()),
        (true, b"explicit cover".as_slice()),
    ] {
        let mut config = conversion(&book, &["--upscale"]);
        if named {
            config.cli.cover = Some(explicit.clone());
        }
        let prepared = prepare(
            input(vec![entry("001.png", b"page".to_vec())]),
            &config,
            "Title",
            true,
            &sink,
            &mut failure(),
        )
        .unwrap();
        assert_eq!(prepared.custom_cover.as_deref(), Some(expected));
        assert_eq!(prepared.cover_source.as_deref(), Some(expected));
        assert_eq!(prepared.source_chapters[0].pages[0].bytes, b"page");
    }
}

#[test]
fn joining_precedes_cover_selection_counts_and_bookmark_position_mapping_in_both_directions() {
    let directory = tempfile::tempdir().unwrap();
    let labels = directory.path().join("labels.json");
    std::fs::write(&labels, br#"{"spreads":[0]}"#).unwrap();
    for rtl in [false, true] {
        let mut config = conversion(directory.path(), &["--upscale"]);
        config.cli.spreads = Some(labels.clone());
        config.cli.manga_style = rtl;
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let prepared = prepare(
            input(vec![
                entry("First/001.png", png(32, 48, 30)),
                entry("Second/001.png", png(32, 48, 90)),
            ]),
            &config,
            "Title",
            true,
            &sink,
            &mut failure(),
        )
        .unwrap();
        assert_eq!((prepared.total_chapters, prepared.total_pages), (1, 1));
        let page = &prepared.source_chapters[0].pages[0];
        assert!(page.source_path.is_none());
        assert_eq!(page.extension, "png");
        assert_eq!(
            prepared.cover_source.as_deref(),
            Some(page.bytes.as_slice())
        );
        let joined = image::load_from_memory(&page.bytes).unwrap().to_luma8();
        assert_eq!(joined.dimensions(), (64, 48));
        assert_eq!(
            (joined.get_pixel(0, 0)[0], joined.get_pixel(63, 0)[0]),
            if rtl { (90, 30) } else { (30, 90) }
        );
        assert_eq!(prepared.joined_spreads.joined, [0]);
        assert_eq!(
            [0, 1, 2].map(|index| prepared.joined_spreads.position_after(index)),
            [0, 0, 1]
        );
        let output = events(&bytes);
        assert_eq!(output.len(), 1);
        assert_eq!(
            (output[0]["chapters"].as_u64(), output[0]["pages"].as_u64()),
            (Some(1), Some(1))
        );
    }
}

#[test]
fn webtoon_joins_left_to_right_and_has_no_default_cover_but_keeps_an_explicit_one() {
    let directory = tempfile::tempdir().unwrap();
    let labels = directory.path().join("labels.json");
    std::fs::write(&labels, br#"{"spreads":[0]}"#).unwrap();
    let cover = directory.path().join("cover.png");
    std::fs::write(&cover, b"custom cover").unwrap();
    let sink = EventSink::new(false, std::io::sink());
    for custom in [false, true] {
        let mut config = conversion(directory.path(), &["--webtoon", "--manga-style"]);
        config.cli.spreads = Some(labels.clone());
        if custom {
            config.cli.cover = Some(cover.clone());
        }
        let prepared = prepare(
            input(vec![
                entry("001.png", png(32, 48, 30)),
                entry("002.png", png(32, 48, 90)),
            ]),
            &config,
            "Title",
            true,
            &sink,
            &mut failure(),
        )
        .unwrap();
        let page = image::load_from_memory(&prepared.source_chapters[0].pages[0].bytes)
            .unwrap()
            .to_luma8();
        assert_eq!(
            (page.get_pixel(0, 0)[0], page.get_pixel(63, 0)[0]),
            (30, 90)
        );
        assert_eq!(prepared.total_pages, 1, "webtoon cutting happens later");
        assert_eq!(
            prepared.cover_source.as_deref(),
            custom.then_some(b"custom cover".as_slice())
        );
    }
}

#[test]
fn spread_labels_distinguish_explicit_failure_discovered_warning_and_skipped_pairs() {
    let directory = tempfile::tempdir().unwrap();
    let book = directory.path().join("Source");
    std::fs::create_dir(&book).unwrap();
    let sidecar = directory.path().join("Source.json");
    std::fs::write(&sidecar, b"not json").unwrap();
    let explicit = directory.path().join("explicit.json");
    std::fs::write(&explicit, br#"{"spreads":[0,1,2]}"#).unwrap();
    for mode in ["discovered", "explicit-bad", "explicit-valid"] {
        let mut config = conversion(&book, &["--upscale"]);
        config.cli.spreads = match mode {
            "explicit-bad" => Some(sidecar.clone()),
            "explicit-valid" => Some(explicit.clone()),
            _ => None,
        };
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let mut context = failure();
        let result = prepare(
            input(
                (0..3)
                    .map(|index| entry(&format!("{index}.png"), png(32, 48, 80)))
                    .collect(),
            ),
            &config,
            "Title",
            true,
            &sink,
            &mut context,
        );
        let output = events(&bytes);
        match mode {
            "explicit-bad" => {
                assert!(result.is_err());
                assert_eq!(context.code, "spread_labels_read_failed");
                assert_eq!(
                    context.path.as_deref(),
                    Some(absolute_display(&sidecar).as_str())
                );
                assert!(output.is_empty());
            }
            "explicit-valid" => {
                let prepared = result.unwrap();
                assert_eq!(prepared.total_pages, 2);
                assert_eq!(warnings(&output), ["spread_labels_skipped"]);
                assert_eq!(output[0]["path"], absolute_display(&explicit));
                assert_eq!(output[0]["message"], "Some labelled spreads could not be joined: position 1 is already the second half of a pair; position 2 has no page after it.");
            }
            _ => {
                assert_eq!(result.unwrap().total_pages, 3);
                assert_eq!(warnings(&output), ["spread_labels_ignored"]);
                assert_eq!(output[0]["path"], absolute_display(&sidecar));
            }
        }
    }
}

#[test]
fn cover_read_failure_precedes_spread_read_failure_then_join_failure_keeps_label_path() {
    let directory = tempfile::tempdir().unwrap();
    let bad_cover = directory.path().join("missing.png");
    let labels = directory.path().join("labels.json");
    let sink = EventSink::new(false, std::io::sink());
    for (cover, valid_labels, code, path) in [
        (true, false, "cover_read_failed", &bad_cover),
        (false, false, "spread_labels_read_failed", &labels),
        (false, true, "spread_join_failed", &labels),
    ] {
        if valid_labels {
            std::fs::write(&labels, br#"{"spreads":[0]}"#).unwrap();
        }
        let mut config = conversion(directory.path(), &["--upscale"]);
        config.cli.spreads = Some(labels.clone());
        if cover {
            config.cli.cover = Some(bad_cover.clone());
        }
        let mut context = failure();
        let error = prepare(
            input(vec![
                entry("001.png", b"not a png".to_vec()),
                entry("002.png", png(32, 48, 80)),
            ]),
            &config,
            "Title",
            true,
            &sink,
            &mut context,
        )
        .err()
        .unwrap();
        assert_eq!(context.code, code);
        assert_eq!(context.stage, "inspect");
        assert!(context.recoverable);
        assert_eq!(
            context.path.as_deref(),
            Some(absolute_display(path).as_str())
        );
        assert_eq!(error.to_string(), context.diagnostic);
        assert!(context.manga.is_none());
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
fn warning_and_inspect_completion_write_failures_remain_protocol_errors() {
    let directory = tempfile::tempdir().unwrap();
    let book = directory.path().join("Source");
    std::fs::create_dir(&book).unwrap();
    std::fs::write(directory.path().join("Source.json"), b"not json").unwrap();
    let config = conversion(&book, &[]);
    // Non-image, already-converted, size, ignored labels, inspect-completed.
    for accepted in 0..5 {
        let sink = EventSink::new(
            true,
            FailAfterEvents {
                remaining: accepted,
            },
        );
        let mut context = failure();
        let error = prepare(
            input(vec![
                entry("001-kcc.png", png(32, 48, 80)),
                entry("notes.txt", vec![]),
            ]),
            &config,
            "Title",
            true,
            &sink,
            &mut context,
        )
        .err()
        .unwrap();
        let protocol = error.downcast_ref::<RunFailure>().unwrap();
        assert_eq!(
            protocol.code, "event_write_failed",
            "after {accepted} events"
        );
        assert_eq!(protocol.stage, "protocol");
        assert!(!protocol.recoverable);
        assert_eq!(context.code, "conversion_failed");
    }
}
