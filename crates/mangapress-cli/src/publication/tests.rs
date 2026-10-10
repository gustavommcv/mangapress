use super::*;
use clap::Parser;
use serde_json::Value;
use std::io::{self, Write};
use std::path::Path;

fn conversion(input: &Path, profile: &str, format: &str) -> ResolvedConversion {
    let cli = crate::args::Cli::parse_from(
        [std::ffi::OsStr::new("mangapress"), input.as_os_str()]
            .into_iter()
            .chain(
                [
                    "--profile",
                    profile,
                    "--format",
                    format,
                    "--customwidth",
                    "32",
                    "--customheight",
                    "48",
                ]
                .iter()
                .map(std::ffi::OsStr::new),
            ),
    );
    crate::configuration::resolve(cli, &mut failure()).unwrap()
}

fn failure() -> RunFailure {
    RunFailure::new(
        "page_processing_failed",
        "process",
        false,
        "old message",
        "old diagnostic",
    )
    .with_manga("old manga")
    .with_chapter("old chapter")
    .with_page(7)
    .with_path("old path")
}

fn book() -> AssembledBook {
    AssembledBook {
        bytes: b"complete assembled payload".to_vec(),
        title: "Book & title".into(),
        author: "Named author".into(),
    }
}

fn counts() -> BookCounts {
    BookCounts {
        chapters: 2,
        source_pages: 7,
        output_pages: 9,
    }
}

fn planned(directory: &Path, format: &'static str, extension: &'static str) -> PlannedOutput {
    let output_path = directory.join(format!("book.{extension}"));
    PlannedOutput {
        staged_output: Some(crate::output::StagedOutput::new(&output_path).unwrap()),
        output_path_absolute: output_path.to_string_lossy().into_owned(),
        output_path,
        format,
        extension,
    }
}

fn events(log: &[u8]) -> Vec<Value> {
    std::str::from_utf8(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_write_context(context: &RunFailure, destination: &Path) {
    assert_eq!(context.code, "output_write_failed");
    assert_eq!(context.stage, "write");
    assert!(context.recoverable);
    assert_eq!(context.message, "Couldn't save the converted book.");
    assert_eq!(
        context.diagnostic,
        format!("writing output to {}", destination.display())
    );
    assert_eq!(context.manga.as_deref(), Some("Book & title"));
    assert_eq!(context.path.as_deref(), Some(destination.to_str().unwrap()));
    assert!(context.chapter.is_none() && context.volume.is_none() && context.page.is_none());
}

#[test]
fn publication_keeps_complete_bytes_result_fields_counts_and_format_not_extension() {
    for (profile, format, extension) in [
        ("K11", "epub", "epub"),
        ("K11", "cbz", "cbz"),
        ("K11", "pdf", "pdf"),
        ("KoLC", "epub", "kepub.epub"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let config = conversion(directory.path(), profile, format);
        let output = planned(directory.path(), format, extension);
        let destination = output.output_path.clone();
        let expected = book();
        let bytes = expected.bytes.clone();
        let mut context = failure();
        let mut log = Vec::new();
        publish(
            expected,
            output,
            &config,
            counts(),
            true,
            &EventSink::new(true, &mut log),
            &mut context,
        )
        .unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_write_context(&context, &destination);
        let envelope = |sequence, kind| json!({"protocol_version": 1, "tool": "mangapress", "tool_version": env!("CARGO_PKG_VERSION"), "sequence": sequence, "type": kind});
        let mut started = envelope(1, "stage");
        started.as_object_mut().unwrap().extend(
            json!({"stage":"write", "state":"started", "manga":"Book & title", "path":destination})
                .as_object()
                .unwrap()
                .clone(),
        );
        let mut completed = envelope(2, "stage");
        completed.as_object_mut().unwrap().extend(json!({"stage":"write", "state":"completed", "manga":"Book & title", "path":destination, "bytes":bytes.len()}).as_object().unwrap().clone());
        let mut result = envelope(3, "result");
        result.as_object_mut().unwrap().extend(json!({"status":"completed", "operation":"convert", "dry_run":false, "manga":"Book & title", "author":"Named author", "format":format, "profile":profile, "width":32, "height":48, "chapters":2, "source_pages":7, "output_pages":9, "output_path":destination, "bytes":bytes.len(), "written":true}).as_object().unwrap().clone());
        assert_eq!(events(&log), [started, completed, result]);
    }
}

struct ObservePublication<'a> {
    destination: &'a Path,
    log: Vec<u8>,
    states: Vec<(String, bool, usize)>,
}

impl Write for ObservePublication<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.log.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let values = events(&self.log);
        let last = values.last().unwrap();
        let label = last
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("result");
        let siblings = std::fs::read_dir(self.destination.parent().unwrap())?.count();
        self.states
            .push((label.into(), self.destination.exists(), siblings));
        if self.destination.exists() {
            assert_eq!(std::fs::read(self.destination)?, book().bytes);
        }
        Ok(())
    }
}

#[test]
fn write_completion_and_result_follow_publication_and_temporary_cleanup() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "cbz");
    let output = planned(directory.path(), "cbz", "cbz");
    let destination = output.output_path.clone();
    let mut observer = ObservePublication {
        destination: &destination,
        log: Vec::new(),
        states: Vec::new(),
    };
    publish(
        book(),
        output,
        &config,
        counts(),
        true,
        &EventSink::new(true, &mut observer),
        &mut failure(),
    )
    .unwrap();
    assert_eq!(
        observer.states,
        [
            ("started".into(), false, 1),
            ("completed".into(), true, 1),
            ("result".into(), true, 1)
        ]
    );
}

#[test]
fn a_file_winner_after_staging_survives_without_completed_write_or_result() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "epub");
    let output = planned(directory.path(), "epub", "epub");
    let destination = output.output_path.clone();
    std::fs::write(&destination, b"another process's book").unwrap();
    let mut log = Vec::new();
    let mut context = failure();
    let error = publish(
        book(),
        output,
        &config,
        counts(),
        true,
        &EventSink::new(true, &mut log),
        &mut context,
    )
    .unwrap_err();
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        b"another process's book"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_write_context(&context, &destination);
    assert_eq!(
        error.to_string(),
        format!("writing output to {}", destination.display())
    );
    assert_eq!(
        error.downcast_ref::<io::Error>().unwrap().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(events(&log).len(), 1);
    assert_eq!(events(&log)[0]["state"], "started");
}

#[test]
fn a_directory_winner_after_staging_keeps_its_contents_and_cleans_our_temporary() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "epub");
    let output = planned(directory.path(), "epub", "epub");
    let destination = output.output_path.clone();
    std::fs::create_dir(&destination).unwrap();
    let sentinel = destination.join("keep.txt");
    std::fs::write(&sentinel, b"keep directory contents").unwrap();
    let mut log = Vec::new();
    let mut context = failure();
    let error = publish(
        book(),
        output,
        &config,
        counts(),
        true,
        &EventSink::new(true, &mut log),
        &mut context,
    )
    .unwrap_err();
    assert!(error.downcast_ref::<io::Error>().is_some());
    assert_write_context(&context, &destination);
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"keep directory contents"
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(events(&log).len(), 1);
}

struct StopWriter {
    accepted: usize,
    fail_flush: bool,
    kind: io::ErrorKind,
    log: Vec<u8>,
}

impl Write for StopWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.accepted == 0 && !self.fail_flush {
            return Err(io::Error::new(self.kind, "publication test writer stopped"));
        }
        self.log.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.accepted == 0 {
            return Err(io::Error::new(self.kind, "publication test flush stopped"));
        }
        self.accepted -= 1;
        Ok(())
    }
}

fn assert_protocol_failure(error: &anyhow::Error) {
    let protocol = error.downcast_ref::<RunFailure>().unwrap();
    assert_eq!(protocol.code, "event_write_failed");
    assert_eq!(protocol.stage, "protocol");
    assert!(!protocol.recoverable);
}

#[test]
fn a_started_event_write_failure_drops_staging_without_publishing() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "cbz");
    let output = planned(directory.path(), "cbz", "cbz");
    let destination = output.output_path.clone();
    let mut writer = StopWriter {
        accepted: 0,
        fail_flush: false,
        kind: io::ErrorKind::Other,
        log: Vec::new(),
    };
    let mut context = failure();
    let error = publish(
        book(),
        output,
        &config,
        counts(),
        true,
        &EventSink::new(true, &mut writer),
        &mut context,
    )
    .unwrap_err();
    assert_protocol_failure(&error);
    assert_write_context(&context, &destination);
    assert!(writer.log.is_empty());
    assert!(!destination.exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn event_write_failures_after_publication_leave_the_complete_book_in_place() {
    for accepted in 1..=2 {
        let directory = tempfile::tempdir().unwrap();
        let config = conversion(directory.path(), "K11", "cbz");
        let output = planned(directory.path(), "cbz", "cbz");
        let destination = output.output_path.clone();
        let mut writer = StopWriter {
            accepted,
            fail_flush: false,
            kind: io::ErrorKind::BrokenPipe,
            log: Vec::new(),
        };
        let mut context = failure();
        let error = publish(
            book(),
            output,
            &config,
            counts(),
            true,
            &EventSink::new(true, &mut writer),
            &mut context,
        )
        .unwrap_err();
        assert_protocol_failure(&error);
        assert_write_context(&context, &destination);
        assert_eq!(std::fs::read(&destination).unwrap(), book().bytes);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        let emitted = events(&writer.log);
        assert_eq!(emitted.len(), accepted);
        assert!(emitted.iter().all(|e| e["type"] == "stage"));
        assert_eq!(
            emitted.last().unwrap()["state"],
            if accepted == 1 {
                "started"
            } else {
                "completed"
            }
        );
    }
}

#[test]
fn flush_failures_remain_fatal_before_and_after_publication() {
    for accepted in 0..=2 {
        let directory = tempfile::tempdir().unwrap();
        let config = conversion(directory.path(), "K11", "cbz");
        let output = planned(directory.path(), "cbz", "cbz");
        let destination = output.output_path.clone();
        let mut writer = StopWriter {
            accepted,
            fail_flush: true,
            kind: io::ErrorKind::BrokenPipe,
            log: Vec::new(),
        };
        let error = publish(
            book(),
            output,
            &config,
            counts(),
            true,
            &EventSink::new(true, &mut writer),
            &mut failure(),
        )
        .unwrap_err();
        assert_protocol_failure(&error);
        assert_eq!(destination.exists(), accepted > 0);
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            usize::from(accepted > 0)
        );
        if accepted > 0 {
            assert_eq!(std::fs::read(&destination).unwrap(), book().bytes);
        }
        // A complete line can reach a reader even when its final flush fails.
        assert_eq!(events(&writer.log).len(), accepted + 1);
    }
}

#[test]
fn a_disabled_event_sink_does_not_touch_its_writer_or_block_publication() {
    let directory = tempfile::tempdir().unwrap();
    let config = conversion(directory.path(), "K11", "pdf");
    let output = planned(directory.path(), "pdf", "pdf");
    let destination = output.output_path.clone();
    let mut writer = StopWriter {
        accepted: 0,
        fail_flush: false,
        kind: io::ErrorKind::Other,
        log: Vec::new(),
    };
    publish(
        book(),
        output,
        &config,
        counts(),
        true,
        &EventSink::new(false, &mut writer),
        &mut failure(),
    )
    .unwrap();
    assert!(writer.log.is_empty());
    assert_eq!(std::fs::read(destination).unwrap(), book().bytes);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
