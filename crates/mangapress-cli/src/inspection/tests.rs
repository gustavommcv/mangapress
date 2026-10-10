use super::*;
use clap::Parser;
use serde_json::Value;
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

fn stages(events: &[Value]) -> Vec<(&str, &str)> {
    events
        .iter()
        .filter(|event| event["type"] == "stage")
        .map(|event| {
            (
                event["stage"].as_str().unwrap(),
                event["state"].as_str().unwrap(),
            )
        })
        .collect()
}

#[test]
fn reading_retains_entries_diagnostics_raw_xml_and_resolved_metadata_without_processing() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("Source");
    std::fs::create_dir_all(input.join("Chapter 2")).unwrap();
    // Reading does not decode page images or consume their bytes.
    std::fs::write(input.join("001.png"), b"first untouched page").unwrap();
    std::fs::write(input.join("Chapter 2/002.png"), b"second untouched page").unwrap();
    std::fs::write(input.join("Chapter 2/ComicInfo.xml"), b"nested metadata").unwrap();
    std::fs::write(input.join("notes.txt"), b"notes").unwrap();
    let xml = br#"<ComicInfo><Series>Series</Series><Volume>3</Volume><Title>Story</Title><Writer>Bea, Ann</Writer><Summary>Summary</Summary><Pages><Page Image="1" Bookmark="Start"/></Pages></ComicInfo>"#;
    std::fs::write(input.join("ComicInfo.xml"), xml).unwrap();
    let config = conversion(&input, &[]);
    let mut context = failure();
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let result = read(&config, true, &sink, &mut context).unwrap();

    assert_eq!(result.book_input.entries.len(), 2);
    for (entry, path, data) in [
        (
            &result.book_input.entries[0],
            "001.png",
            b"first untouched page".as_slice(),
        ),
        (
            &result.book_input.entries[1],
            "Chapter 2/002.png",
            b"second untouched page".as_slice(),
        ),
    ] {
        assert_eq!(entry.relative_path, Path::new(path));
        assert_eq!(entry.bytes, data);
    }
    assert_eq!(result.book_input.skipped_non_images, 2);
    assert!(result.book_input.skipped_links.is_empty());
    assert_eq!(
        result.metadata.comic_info_xml.as_deref(),
        Some(xml.as_slice())
    );
    assert_eq!(
        result.metadata.comic_info.unwrap().bookmarks,
        vec![(1, "Start".to_string())]
    );
    assert_eq!(result.metadata.resolved.title, "Series Vol. 03");
    assert_eq!(result.metadata.resolved.authors, ["Ann", "Bea"]);
    assert_eq!(result.metadata.author, "Ann, Bea");
    assert_eq!(result.metadata.resolved.summary.as_deref(), Some("Summary"));
    assert_eq!(result.metadata.resolved.series.as_deref(), Some("Series"));
    assert_eq!(
        result.metadata.resolved.series_position.as_deref(),
        Some("3")
    );
    // Successful reads keep the existing failure context; later stages replace it.
    assert_eq!(context.code, "input_read_failed");
    assert_eq!(context.path.as_deref(), Some(config.input_path.as_str()));
    let events = events(&bytes);
    assert_eq!(
        stages(&events),
        [
            ("inspect", "started"),
            ("metadata", "started"),
            ("metadata", "completed")
        ]
    );
    assert_eq!(
        events.len(),
        3,
        "filtering and inspect-completion are later steps"
    );
    assert_eq!(events[0]["path"], config.input_path);
    assert_eq!(events[2]["title"], "Series Vol. 03");
    assert_eq!(events[2]["author"], "Ann, Bea");
    assert_eq!(events[2]["comic_info_found"], true);
    assert_eq!(std::fs::read(input.join("ComicInfo.xml")).unwrap(), xml);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn metadata_modes_and_explicit_options_reach_the_existing_resolver() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("ComicInfo.xml"), b"<ComicInfo><Series>Series</Series><Volume>3</Volume><Title>Story</Title><Writer>Ann</Writer></ComicInfo>").unwrap();
    for (flags, title, author, position) in [
        (vec![], "Series Vol. 03", "Ann", Some("3")),
        (
            vec!["--metadatatitle", "combine"],
            "Series Vol. 03: Story",
            "Ann",
            Some("3"),
        ),
        (
            vec!["--metadatatitle", "title-only", "--title", "Explicit"],
            "Story",
            "Ann",
            None,
        ),
        (
            vec!["--title", "Explicit", "--author", "Custom"],
            "Explicit",
            "Custom",
            None,
        ),
    ] {
        let config = conversion(directory.path(), &flags);
        let sink = EventSink::new(false, std::io::sink());
        let result = read(&config, true, &sink, &mut failure()).unwrap();
        assert_eq!(result.metadata.resolved.title, title, "{flags:?}");
        assert_eq!(result.metadata.author, author, "{flags:?}");
        assert_eq!(
            result.metadata.resolved.series_position.as_deref(),
            position
        );
        assert!(
            result.book_input.entries.is_empty(),
            "page validation remains downstream"
        );
    }
}

#[test]
fn unreadable_metadata_warns_in_order_but_retains_raw_xml_and_fallback_title() {
    for xml in [None, Some(b"not xml".as_slice())] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("Fallback Title");
        std::fs::create_dir(&input).unwrap();
        std::fs::write(input.join("001.png"), b"page").unwrap();
        if let Some(xml) = xml {
            std::fs::write(input.join("ComicInfo.xml"), xml).unwrap();
        }
        let config = conversion(&input, &[]);
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let result = read(&config, true, &sink, &mut failure()).unwrap();
        assert!(result.metadata.comic_info.is_none());
        assert_eq!(result.metadata.comic_info_xml.as_deref(), xml);
        assert_eq!(result.metadata.resolved.title, "Fallback Title");
        assert_eq!(result.metadata.author, "Unknown");
        assert_eq!(result.book_input.entries.len(), 1);
        let events = events(&bytes);
        assert_eq!(events.len(), 3 + usize::from(xml.is_some()));
        assert_eq!(
            stages(&events),
            [
                ("inspect", "started"),
                ("metadata", "started"),
                ("metadata", "completed")
            ]
        );
        if xml.is_some() {
            assert_eq!(events[2]["type"], "warning");
            assert_eq!(events[2]["code"], "comic_info_unreadable");
            assert_eq!(events[2]["stage"], "metadata");
            assert_eq!(events[2]["path"], config.input_path);
            assert_eq!(events[2]["recoverable"], true);
        }
        assert_eq!(events.last().unwrap()["comic_info_found"], false);
    }
}

#[test]
fn only_truly_empty_input_is_rejected_before_metadata() {
    for file in [
        None,
        Some(("notes.txt", b"notes".as_slice())),
        Some(("ComicInfo.xml", b"<ComicInfo/>".as_slice())),
    ] {
        let directory = tempfile::tempdir().unwrap();
        if let Some((name, data)) = file {
            std::fs::write(directory.path().join(name), data).unwrap();
        }
        let config = conversion(directory.path(), &[]);
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let mut context = failure();
        let result = read(&config, true, &sink, &mut context);
        let events = events(&bytes);
        match file {
            None => {
                assert_eq!(context.code, "input_empty");
                assert_eq!(context.path.as_deref(), Some(config.input_path.as_str()));
                assert!(result.err().unwrap().to_string().contains("no files found"));
                assert_eq!(stages(&events), [("inspect", "started")]);
            }
            Some((name, _)) => {
                let result = result.unwrap();
                assert!(result.book_input.entries.is_empty());
                assert_eq!(
                    result.book_input.skipped_non_images,
                    usize::from(name == "notes.txt")
                );
                assert_eq!(context.code, "input_read_failed");
                assert_eq!(
                    stages(&events),
                    [
                        ("inspect", "started"),
                        ("metadata", "started"),
                        ("metadata", "completed")
                    ]
                );
            }
        }
    }
}

#[test]
fn archive_read_failure_precedes_metadata_and_keeps_its_input_path() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("broken.cbz");
    std::fs::write(&input, b"not a zip").unwrap();
    let config = conversion(&input, &[]);
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    let error = read(&config, true, &sink, &mut context).err().unwrap();
    assert!(format!("{error:#}").contains("reading input from"));
    assert_eq!(context.code, "input_read_failed");
    assert_eq!(context.stage, "inspect");
    assert!(context.recoverable);
    assert_eq!(context.path.as_deref(), Some(config.input_path.as_str()));
    assert_eq!(stages(&events(&bytes)), [("inspect", "started")]);
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
fn failed_inspection_events_remain_protocol_failures_not_metadata_warnings_or_read_errors() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("001.png"), b"page").unwrap();
    std::fs::write(directory.path().join("ComicInfo.xml"), b"not xml").unwrap();
    let config = conversion(directory.path(), &[]);
    // inspect-start, metadata-start, soft warning, metadata-completion.
    for accepted in 0..4 {
        let sink = EventSink::new(
            true,
            FailAfterEvents {
                remaining: accepted,
            },
        );
        let mut context = failure();
        let error = read(&config, true, &sink, &mut context).err().unwrap();
        let protocol = error.downcast_ref::<RunFailure>().unwrap();
        assert_eq!(
            protocol.code, "event_write_failed",
            "after {accepted} events"
        );
        assert_eq!(protocol.stage, "protocol");
        assert!(!protocol.recoverable);
        assert!(protocol.diagnostic.contains("writing JSON event"));
        assert_eq!(
            context.code,
            if accepted == 0 {
                "conversion_failed"
            } else {
                "input_read_failed"
            }
        );
    }
}
