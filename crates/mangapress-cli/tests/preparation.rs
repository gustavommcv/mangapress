mod support;

use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn dry_run_reports_post_join_counts_in_every_mode_without_staging_output() {
    let input = fixture_folder();
    let destination = tempfile::tempdir().unwrap();
    let labels = destination.path().join("labels.json");
    std::fs::write(&labels, br#"{"spreads":[0,1,3]}"#).unwrap();
    let paths = [
        "c001 - One/p0001.png",
        "c001 - One/p0002.png",
        "c002 - Two/p0001.png",
        "c002 - Two/p0002.png",
    ];
    let snapshot = || paths.map(|path| std::fs::read(input.path().join(path)).unwrap());
    let before = snapshot();
    for (quiet, machine) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args(["--dry-run", "--upscale", "--spreads"])
            .arg(&labels)
            .arg("--output")
            .arg(destination.path().join("not-created/book.epub"));
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
            let sequence: Vec<_> = events
                .iter()
                .map(|event| {
                    (
                        event["type"].as_str().unwrap(),
                        event["stage"].as_str(),
                        event["state"].as_str().or_else(|| event["code"].as_str()),
                    )
                })
                .collect();
            assert_eq!(
                sequence,
                [
                    ("stage", Some("inspect"), Some("started")),
                    ("stage", Some("metadata"), Some("started")),
                    ("stage", Some("metadata"), Some("completed")),
                    ("warning", Some("inspect"), Some("spread_labels_skipped")),
                    ("stage", Some("inspect"), Some("completed")),
                    ("stage", Some("plan"), Some("started")),
                    ("stage", Some("plan"), Some("completed")),
                    ("result", None, None),
                ]
            );
            assert_eq!(events[4]["chapters"], 2);
            assert_eq!(events[4]["pages"], 3);
            assert_eq!(events.last().unwrap()["source_pages"], 3);
            assert_eq!(events.last().unwrap()["chapters"], 2);
            assert_eq!(events.last().unwrap()["written"], false);
            assert!(output.stderr.is_empty());
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                stderr
                    .matches("Some labelled spreads could not be joined")
                    .count(),
                1
            );
            assert_eq!(stderr.contains("joined 1 labelled spread"), !quiet);
            assert_eq!(
                stderr.contains("found 2 chapter(s), 3 page(s) total"),
                !quiet
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("dry run -- no pages"));
        }
        assert_eq!(snapshot(), before);
        assert!(!destination.path().join("not-created").exists());
    }
}

#[test]
fn discovered_bad_labels_are_soft_but_explicit_labels_and_cover_fail_before_planning() {
    let directory = tempfile::tempdir().unwrap();
    let book = directory.path().join("Source");
    std::fs::create_dir(&book).unwrap();
    support::write_png(&book.join("001.png"), 80);
    let sidecar = directory.path().join("Source.json");
    std::fs::write(&sidecar, b"not json").unwrap();
    for (named, cover, expected) in [
        (false, false, None),
        (true, false, Some("spread_labels_read_failed")),
        (true, true, Some("cover_read_failed")),
    ] {
        for machine in [false, true] {
            let mut command = Command::new(binary());
            command
                .arg(&book)
                .args(["--dry-run", "--upscale", "--quiet", "--output"])
                .arg(directory.path().join("not-created/book.epub"));
            if named {
                command.arg("--spreads").arg(&sidecar);
            }
            if cover {
                command
                    .arg("--cover")
                    .arg(directory.path().join("missing.png"));
            }
            if machine {
                command.arg("--json-events");
            }
            let output = command.output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(if expected.is_some() { 1 } else { 0 })
            );
            if machine {
                let events = parse_events(&output);
                if let Some(code) = expected {
                    assert_eq!(events.last().unwrap()["code"], code);
                    assert!(!events.iter().any(|event| event["stage"] == "plan"));
                    assert!(!events.iter().any(|event| event["type"] == "warning"));
                } else {
                    assert_eq!(events[3]["code"], "spread_labels_ignored");
                    assert_eq!(events.last().unwrap()["type"], "result");
                }
            } else if let Some(code) = expected {
                let text = String::from_utf8_lossy(&output.stderr);
                assert!(
                    text.contains(if code == "cover_read_failed" {
                        "reading cover image"
                    } else {
                        "reading spread labels"
                    }),
                    "{text}"
                );
                assert!(output.stdout.is_empty());
            } else {
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("not a list of spread labels")
                );
            }
            assert!(!directory.path().join("not-created").exists());
        }
    }
}
