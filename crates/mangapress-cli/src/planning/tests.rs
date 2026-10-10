use super::*;
use clap::Parser;
use serde_json::Value;

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

fn book() -> BookSummary<'static> {
    BookSummary {
        title: "Resolved title",
        author: "Ann, Bea",
        chapters: 2,
        pages: 7,
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

fn converted(outcome: Outcome) -> PlannedOutput {
    match outcome {
        Outcome::Convert(planned) => planned,
        Outcome::DryRun => panic!("expected conversion"),
    }
}

#[test]
fn dry_run_reports_resolved_book_and_plan_without_creating_parents_or_staging() {
    let directory = tempfile::tempdir().unwrap();
    let mut config = conversion(
        directory.path(),
        &[
            "--dry-run",
            "--profile",
            "K11",
            "--customwidth",
            "120",
            "--customheight",
            "180",
        ],
    );
    let output_path = directory
        .path()
        .join("missing")
        .join("nested")
        .join("book.epub");
    config.cli.output = Some(output_path.clone());
    let mut bytes = Vec::new();
    let sink = EventSink::new(true, &mut bytes);
    let mut context = failure();
    assert!(matches!(
        prepare(&config, &book(), &sink, &mut context).unwrap(),
        Outcome::DryRun
    ));
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert_eq!(context.code, "output_plan_failed");
    assert_eq!(
        context.path.as_deref(),
        Some(absolute_display(&output_path).as_str())
    );
    let output = events(&bytes);
    assert_eq!(output.len(), 3);
    assert_eq!(output[0]["stage"], "plan");
    assert_eq!(output[0]["state"], "started");
    assert_eq!(output[1]["state"], "completed");
    assert_eq!(output[1]["chapters"], 2);
    assert_eq!(output[1]["pages"], 7);
    assert_eq!(output[1]["output_path"], absolute_display(&output_path));
    assert_eq!(output[1]["gray_levels"], 16);
    let result = &output[2];
    assert_eq!(result["type"], "result");
    assert_eq!(result["manga"], "Resolved title");
    assert_eq!(result["author"], "Ann, Bea");
    assert_eq!(result["width"], 120);
    assert_eq!(result["height"], 180);
    assert_eq!(result["source_pages"], 7);
    assert_eq!(result["chapters"], 2);
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["written"], false);
}

#[test]
fn output_extension_and_protocol_format_remain_distinct_for_kobo_and_other_modes() {
    for (flags, extension, format) in [
        (vec!["--profile", "KoC"], "kepub.epub", "epub"),
        (vec!["--profile", "KoC", "--nokepub"], "epub", "epub"),
        (
            vec!["--profile", "KoC", "--customwidth", "700"],
            "epub",
            "epub",
        ),
        (vec!["--profile", "KoC", "--format", "cbz"], "cbz", "cbz"),
        (vec!["--profile", "K11", "--format", "epub"], "epub", "epub"),
        (vec!["--profile", "KDX"], "cbz", "cbz"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("Source.with.dots");
        std::fs::create_dir(&input).unwrap();
        let flags: Vec<_> = flags.into_iter().chain(["--dry-run"]).collect();
        let config = conversion(&input, &flags);
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        assert!(matches!(
            prepare(&config, &book(), &sink, &mut failure()).unwrap(),
            Outcome::DryRun
        ));
        let output = events(&bytes);
        for event in &output[1..] {
            assert_eq!(event["format"], format, "{flags:?}");
            assert_eq!(
                event["output_path"],
                absolute_display(
                    &directory
                        .path()
                        .join(format!("Source.with.dots.{extension}"))
                )
            );
            assert_eq!(event["width"], config.width);
            assert_eq!(event["height"], config.height);
        }
        {
            let mut config = config;
            config.cli.dry_run = false;
            let sink = EventSink::new(false, std::io::sink());
            let planned = converted(prepare(&config, &book(), &sink, &mut failure()).unwrap());
            assert_eq!(planned.format, format);
            assert_eq!(
                planned.extension, extension,
                "later package diagnostics keep this extension"
            );
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn conversion_stages_before_completion_and_returns_the_existing_publication_handle() {
    struct StagingWitness<'a> {
        bytes: &'a mut Vec<u8>,
        destination: &'a Path,
    }
    impl std::io::Write for StagingWitness<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            let event: Value = serde_json::from_str(
                std::str::from_utf8(self.bytes)
                    .unwrap()
                    .lines()
                    .last()
                    .unwrap(),
            )
            .unwrap();
            if event["stage"] == "plan" && event["state"] == "completed" {
                assert!(!self.destination.exists());
                let files: Vec<_> = std::fs::read_dir(self.destination.parent().unwrap())?
                    .map(|entry| entry.unwrap().file_name())
                    .collect();
                assert_eq!(
                    files.len(),
                    1,
                    "staging must exist when completion is emitted"
                );
                assert!(files[0].to_string_lossy().starts_with(".mangapress-"));
            }
            Ok(())
        }
    }
    for publish in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory
            .path()
            .join("missing")
            .join("books")
            .join("book.epub");
        {
            let mut config = conversion(directory.path(), &["--format", "epub"]);
            config.cli.output = Some(destination.clone());
            let mut bytes = Vec::new();
            let sink = EventSink::new(
                true,
                StagingWitness {
                    bytes: &mut bytes,
                    destination: &destination,
                },
            );
            let planned = converted(prepare(&config, &book(), &sink, &mut failure()).unwrap());
            assert_eq!(planned.output_path, destination);
            assert_eq!(planned.output_path_absolute, absolute_display(&destination));
            assert_eq!(planned.format, "epub");
            assert_eq!(planned.extension, "epub");
            assert!(planned.staged_output.is_some());
            assert!(!destination.exists());
            let files: Vec<_> = std::fs::read_dir(destination.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(files.len(), 1);
            assert!(files[0].to_string_lossy().starts_with(".mangapress-"));
            let output = events(&bytes);
            assert_eq!(
                output.len(),
                2,
                "no result emitted before actual conversion"
            );
            assert_eq!(output[1]["state"], "completed");
            if publish {
                planned
                    .staged_output
                    .unwrap()
                    .write(b"complete book")
                    .unwrap();
            }
        }
        if publish {
            assert_eq!(std::fs::read(&destination).unwrap(), b"complete book");
            assert_eq!(
                std::fs::read_dir(destination.parent().unwrap())
                    .unwrap()
                    .count(),
                1
            );
        } else {
            assert!(!destination.exists());
            assert_eq!(
                std::fs::read_dir(destination.parent().unwrap())
                    .unwrap()
                    .count(),
                0
            );
        }
    }
}

#[test]
fn destination_validation_keeps_plan_failure_context_and_never_emits_completion() {
    for blocked_parent in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut config = conversion(directory.path(), &["--dry-run"]);
        let destination = if blocked_parent {
            let blocked = directory.path().join("blocked");
            std::fs::write(&blocked, b"keep parent file").unwrap();
            blocked.join("book.epub")
        } else {
            directory.path().join("missing/NUL.epub")
        };
        config.cli.output = Some(destination.clone());
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        let mut context = failure();
        let error = prepare(&config, &book(), &sink, &mut context)
            .err()
            .unwrap();
        assert!(format!("{error:#}").starts_with("planning the output destination:"));
        assert_eq!(context.code, "output_plan_failed");
        assert_eq!(context.stage, "plan");
        assert_eq!(context.manga.as_deref(), Some("Resolved title"));
        assert_eq!(
            context.path.as_deref(),
            Some(absolute_display(&destination).as_str())
        );
        assert!(context.recoverable);
        assert_eq!(events(&bytes).len(), 1);
        assert!(!directory.path().join("missing").exists());
        if blocked_parent {
            assert_eq!(
                std::fs::read(directory.path().join("blocked")).unwrap(),
                b"keep parent file"
            );
        }
    }
}

#[test]
fn input_and_existing_output_collisions_warn_with_the_actual_safe_path_without_mutation() {
    for input_collision in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("Source.cbz");
        std::fs::write(&input, b"input bytes retained").unwrap();
        let destination = if input_collision {
            input.clone()
        } else {
            directory.path().join("book.cbz")
        };
        std::fs::write(&destination, b"occupied bytes retained").unwrap();
        let before = std::fs::read(&input).unwrap();
        let mut config = conversion(&input, &["--dry-run", "--format", "cbz"]);
        config.cli.output = Some(destination.clone());
        let mut bytes = Vec::new();
        let sink = EventSink::new(true, &mut bytes);
        assert!(matches!(
            prepare(&config, &book(), &sink, &mut failure()).unwrap(),
            Outcome::DryRun
        ));
        let output = events(&bytes);
        assert_eq!(output.len(), 4);
        assert_eq!(output[1]["type"], "warning");
        assert_eq!(
            output[1]["code"],
            if input_collision {
                "output_collision"
            } else {
                "output_exists"
            }
        );
        assert_eq!(output[1]["stage"], "plan");
        assert_eq!(output[1]["manga"], "Resolved title");
        let safe = destination.with_file_name(format!(
            "{} (mangapress).cbz",
            destination.file_stem().unwrap().to_string_lossy()
        ));
        assert_eq!(output[1]["path"], absolute_display(&safe));
        assert_eq!(output[2]["output_path"], absolute_display(&safe));
        assert_eq!(output[3]["output_path"], absolute_display(&safe));
        assert!(!safe.exists());
        assert_eq!(std::fs::read(&input).unwrap(), before);
        assert_eq!(
            std::fs::read(&destination).unwrap(),
            b"occupied bytes retained"
        );
    }
}

#[test]
fn failed_parent_creation_is_a_write_stage_failure_not_a_plan_or_process_error() {
    let directory = tempfile::tempdir().unwrap();
    let blocked = directory.path().join("blocked");
    std::fs::write(&blocked, b"keep file").unwrap();
    let destination = blocked.join("missing/book.epub");
    let mut context = failure();
    let error = stage_destination(
        &destination,
        &absolute_display(&destination),
        "Resolved title",
        &mut context,
    )
    .err()
    .unwrap();
    assert!(format!("{error:#}").starts_with("creating output directory"));
    assert_eq!(context.code, "output_directory_create_failed");
    assert_eq!(context.stage, "write");
    assert!(context.recoverable);
    assert_eq!(
        context.path.as_deref(),
        Some(absolute_display(destination.parent().unwrap()).as_str())
    );
    assert_eq!(context.manga.as_deref(), Some("Resolved title"));
    assert_eq!(std::fs::read(&blocked).unwrap(), b"keep file");
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
fn failed_plan_warning_completion_and_result_events_preserve_protocol_errors_and_cleanup() {
    for dry_run in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("book.epub");
        std::fs::write(&destination, b"keep existing book").unwrap();
        let mut config = conversion(directory.path(), &[]);
        config.cli.output = Some(destination.clone());
        config.cli.dry_run = dry_run;
        // Plan-start, collision warning, plan-completed, optional dry-run result.
        for accepted in 0..if dry_run { 4 } else { 3 } {
            let sink = EventSink::new(
                true,
                FailAfterEvents {
                    remaining: accepted,
                },
            );
            let mut context = failure();
            let error = prepare(&config, &book(), &sink, &mut context)
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
                    "output_plan_failed"
                }
            );
            assert_eq!(std::fs::read(&destination).unwrap(), b"keep existing book");
            assert_eq!(
                std::fs::read_dir(directory.path()).unwrap().count(),
                1,
                "no leaked staging file after {accepted} events"
            );
        }
    }
}
