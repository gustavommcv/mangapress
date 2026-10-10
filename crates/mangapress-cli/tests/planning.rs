mod support;

use std::path::Path;
use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn collision_dry_runs_report_the_same_safe_path_in_every_mode_without_writing() {
    let input = fixture_folder();
    let directory = tempfile::tempdir().unwrap();
    let requested = directory.path().join("book.epub");
    let safe = directory.path().join("book (mangapress).epub");
    std::fs::write(&requested, b"keep existing book").unwrap();
    for (quiet, machine) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args([
                "--dry-run",
                "--upscale",
                "--title",
                "Metadata title",
                "--output",
            ])
            .arg(&requested);
        if quiet {
            command.arg("--quiet");
        }
        if machine {
            command.arg("--json-events");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if machine {
            let events = parse_events(&output);
            let warning = events
                .iter()
                .find(|event| event["type"] == "warning")
                .unwrap();
            assert_eq!(warning["code"], "output_exists");
            assert_eq!(warning["path"], safe.to_string_lossy().as_ref());
            assert_eq!(warning["manga"], "Metadata title");
            let plan = events
                .iter()
                .find(|event| event["stage"] == "plan" && event["state"] == "completed")
                .unwrap();
            assert_eq!(plan["output_path"], safe.to_string_lossy().as_ref());
            let result = events.last().unwrap();
            assert_eq!(result["output_path"], safe.to_string_lossy().as_ref());
            assert_eq!(result["written"], false);
            assert_eq!(result["chapters"], 2);
            assert_eq!(result["source_pages"], 4);
            assert!(output.stderr.is_empty());
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("dry run -- no pages"));
            assert!(stdout.contains("title: Metadata title"));
            assert!(stdout.contains(&format!("would write: {}", safe.display())));
            assert!(String::from_utf8_lossy(&output.stderr)
                .contains("warning: The requested output already exists"));
        }
        assert_eq!(std::fs::read(&requested).unwrap(), b"keep existing book");
        assert!(!safe.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[test]
fn invalid_destination_fails_after_input_warnings_and_inspection_before_processing() {
    let input = fixture_folder();
    std::fs::write(input.path().join("ComicInfo.xml"), b"not xml").unwrap();
    std::fs::write(input.path().join("notes.txt"), b"notes").unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("not-created/NUL.epub");
    for machine in [false, true] {
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args([
                "--quiet",
                "--dry-run",
                "--upscale",
                "--title",
                "Chosen",
                "--output",
            ])
            .arg(&destination);
        if machine {
            command.arg("--json-events");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        if machine {
            let events = parse_events(&output);
            assert_eq!(
                events
                    .iter()
                    .map(|event| event["type"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["stage", "stage", "warning", "stage", "warning", "stage", "stage", "error"]
            );
            assert_eq!(events[2]["code"], "comic_info_unreadable");
            assert_eq!(events[4]["code"], "skipped_non_images");
            assert_eq!(events[5]["state"], "completed");
            assert_eq!(events[6]["stage"], "plan");
            assert_eq!(events[6]["state"], "started");
            let failure = events.last().unwrap();
            assert_eq!(failure["code"], "output_plan_failed");
            assert_eq!(failure["manga"], "Chosen");
            assert_eq!(failure["path"], destination.to_string_lossy().as_ref());
        } else {
            assert!(output.stdout.is_empty());
            let text = String::from_utf8_lossy(&output.stderr);
            assert!(
                text.find("warning: ComicInfo.xml").unwrap()
                    < text.find("warning: skipped").unwrap()
            );
            assert!(text.contains("planning the output destination"));
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

#[test]
fn dry_run_returns_before_webtoon_decode_and_real_failure_cleans_the_staged_file() {
    let input = tempfile::tempdir().unwrap();
    std::fs::write(input.path().join("001.png"), b"unreadable page").unwrap();
    let directory = tempfile::tempdir().unwrap();
    for dry_run in [false, true] {
        for machine in [false, true] {
            let parent = directory.path().join(format!("books-{dry_run}-{machine}"));
            let destination = parent.join("book.epub");
            let mut command = Command::new(binary());
            command
                .arg(input.path())
                .args(["--quiet", "--webtoon", "--output"])
                .arg(&destination);
            if dry_run {
                command.arg("--dry-run");
            }
            if machine {
                command.arg("--json-events");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(if dry_run { 0 } else { 1 }));
            if machine {
                let events = parse_events(&output);
                assert!(events
                    .iter()
                    .any(|event| event["stage"] == "plan" && event["state"] == "completed"));
                assert_eq!(
                    events.last().unwrap()["type"],
                    if dry_run { "result" } else { "error" }
                );
                if dry_run {
                    assert_eq!(events.last().unwrap()["written"], false);
                } else {
                    assert_eq!(events.last().unwrap()["code"], "webtoon_split_failed");
                }
                assert!(!events.iter().any(|event| event["stage"] == "write"));
            } else if dry_run {
                assert!(String::from_utf8_lossy(&output.stdout).contains("dry run -- no pages"));
            }
            assert!(!destination.exists());
            if dry_run {
                assert!(!parent.exists());
            } else {
                assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 0);
            }
            assert_eq!(
                std::fs::read(input.path().join(Path::new("001.png"))).unwrap(),
                b"unreadable page"
            );
        }
    }
}
