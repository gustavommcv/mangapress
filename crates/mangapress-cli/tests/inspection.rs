mod support;

use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn metadata_and_filter_warnings_precede_cover_failure_even_in_quiet_mode() {
    let input = fixture_folder();
    std::fs::write(input.path().join("ComicInfo.xml"), b"not xml").unwrap();
    std::fs::write(input.path().join("notes.txt"), b"notes").unwrap();
    let destination = tempfile::tempdir().unwrap();
    for machine in [false, true] {
        let output = Command::new(binary())
            .arg(input.path())
            .args(["--quiet", "--dry-run", "--cover"])
            .arg(destination.path().join("missing.png"))
            .arg("--output")
            .arg(destination.path().join("not-created/book.epub"))
            .args(if machine {
                vec!["--json-events"]
            } else {
                vec![]
            })
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        if machine {
            let events = parse_events(&output);
            let sequence: Vec<_> = events
                .iter()
                .map(|event| {
                    (
                        event["type"].as_str().unwrap(),
                        event["stage"].as_str().unwrap(),
                        event["code"]
                            .as_str()
                            .or_else(|| event["state"].as_str())
                            .unwrap(),
                    )
                })
                .collect();
            assert_eq!(
                sequence,
                [
                    ("stage", "inspect", "started"),
                    ("stage", "metadata", "started"),
                    ("warning", "metadata", "comic_info_unreadable"),
                    ("stage", "metadata", "completed"),
                    ("warning", "inspect", "skipped_non_images"),
                    ("warning", "inspect", "images_smaller_than_device"),
                    ("error", "inspect", "cover_read_failed"),
                ]
            );
            assert_eq!(events[4]["count"], 1);
            assert_eq!(
                events[6]["path"],
                destination
                    .path()
                    .join("missing.png")
                    .to_string_lossy()
                    .as_ref()
            );
        } else {
            assert!(output.stdout.is_empty());
            let text = String::from_utf8_lossy(&output.stderr);
            let metadata = text.find("warning: ComicInfo.xml").unwrap();
            let filtering = text.find("warning: skipped 1 non-image").unwrap();
            let cover = text.find("reading cover image").unwrap();
            assert!(metadata < filtering && filtering < cover, "{text}");
            assert!(!text.contains("mangapress: converting"));
        }
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
    }
}

#[test]
fn dry_run_inspection_is_read_only_and_keeps_warnings_in_every_reporting_mode() {
    let input = fixture_folder();
    std::fs::write(input.path().join("ComicInfo.xml"), b"not xml").unwrap();
    std::fs::write(input.path().join("notes.txt"), b"notes").unwrap();
    let paths = [
        "ComicInfo.xml",
        "notes.txt",
        "c001 - One/p0001.png",
        "c001 - One/p0002.png",
        "c002 - Two/p0001.png",
        "c002 - Two/p0002.png",
    ];
    let snapshot = || paths.map(|path| std::fs::read(input.path().join(path)).unwrap());
    let before = snapshot();
    let destination = tempfile::tempdir().unwrap();
    for (quiet, machine) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args(["--dry-run", "--upscale", "--output"])
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
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event["type"] == "warning")
                    .map(|event| event["code"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                ["comic_info_unreadable", "skipped_non_images"]
            );
            assert_eq!(events.last().unwrap()["source_pages"], 4);
            assert_eq!(events.last().unwrap()["dry_run"], true);
            assert_eq!(events.last().unwrap()["written"], false);
            assert!(output.stderr.is_empty());
        } else {
            let text = String::from_utf8_lossy(&output.stderr);
            assert_eq!(text.matches("warning: ComicInfo.xml").count(), 1);
            assert_eq!(text.matches("warning: skipped 1 non-image").count(), 1);
            assert_eq!(text.contains("mangapress: converting"), !quiet);
            assert!(String::from_utf8_lossy(&output.stdout).contains("dry run -- no pages"));
        }
        assert_eq!(snapshot(), before);
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(input.path()).unwrap().count(), 4);
    }
}
