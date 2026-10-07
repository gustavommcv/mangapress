#![cfg(any(unix, windows))]

mod support;
mod links {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/folder_links.rs"
    ));
}

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

#[test]
fn folder_link_warnings_and_page_counts_match_in_plan_and_conversion() {
    for root_link in [false, true] {
        for machine in [false, true] {
            for dry_run in [true, false] {
                let parent = tempfile::tempdir().unwrap();
                if !links::supported(parent.path()) {
                    return;
                }
                let root = parent.path().join("Book");
                let chapter = root.join("c001");
                fs::create_dir_all(&chapter).unwrap();
                let page = chapter.join("p1.png");
                write_png(&page, 32);
                links::file_link(Path::new("p1.png"), &chapter.join("p2.png")).unwrap();
                let private = parent.path().join("not-selected-private-image.png");
                write_png(&private, 200);
                links::file_link(&private, &chapter.join("p3.png")).unwrap();
                links::file_link(Path::new("missing.png"), &chapter.join("p4.png")).unwrap();
                links::directory_link(&root, &chapter.join("loop")).unwrap();
                let private_xml = parent.path().join("not-selected-private-metadata.xml");
                fs::write(&private_xml, b"invalid private XML").unwrap();
                links::file_link(&private_xml, &root.join("ComicInfo.xml")).unwrap();
                let input = if root_link {
                    let alias = parent.path().join("shortcut");
                    links::directory_link(&root, &alias).unwrap();
                    alias
                } else {
                    root
                };
                let destination = parent.path().join("output.cbz");
                let mut command = Command::new(binary());
                command
                    .arg(&input)
                    .args(["--format", "cbz", "--noprocessing", "--quiet", "--output"])
                    .arg(&destination);
                if machine {
                    command.arg("--json-events");
                }
                if dry_run {
                    command.arg("--dry-run");
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                for text in [&stdout, &stderr] {
                    assert!(!text.contains("not-selected-private-image"));
                    assert!(!text.contains("not-selected-private-metadata"));
                    assert!(!text.contains("invalid private XML"));
                }
                if machine {
                    assert!(stderr.is_empty());
                    let events = parse_events(&output);
                    let warnings: Vec<_> = events
                        .iter()
                        .filter(|event| event["code"] == "link_skipped")
                        .collect();
                    let expected: Vec<_> =
                        ["c001/loop", "c001/p3.png", "c001/p4.png", "ComicInfo.xml"]
                            .map(|path| input.join(path))
                            .into();
                    let actual: Vec<_> = warnings
                        .iter()
                        .map(|event| {
                            assert_eq!(event["stage"], "inspect");
                            assert_eq!(event["recoverable"], true);
                            assert!(event["message"]
                                .as_str()
                                .unwrap()
                                .starts_with("Skipped a symbolic link because"));
                            PathBuf::from(event["path"].as_str().unwrap())
                        })
                        .collect();
                    assert_eq!(actual, expected);
                    let result = events.last().unwrap();
                    assert_eq!(result["type"], "result");
                    assert_eq!(result["source_pages"], 2);
                    assert_eq!(result["written"], !dry_run);
                    if dry_run {
                        assert!(!events
                            .iter()
                            .any(|event| event["type"] == "page" || event["stage"] == "write"));
                    }
                } else {
                    assert_eq!(stderr.matches("Skipped a symbolic link because").count(), 4);
                    assert!(stderr.contains("leads outside"));
                    assert!(stderr.contains("could not be resolved"));
                    assert!(stderr.contains("regular file"));
                    if !dry_run {
                        assert!(stdout.is_empty());
                    }
                }
                assert_eq!(destination.exists(), !dry_run);
                if !dry_run {
                    let mut archive =
                        zip::ZipArchive::new(fs::File::open(&destination).unwrap()).unwrap();
                    assert_eq!(archive.len(), 2);
                    let original = fs::read(&page).unwrap();
                    for index in 0..archive.len() {
                        let mut bytes = Vec::new();
                        std::io::Read::read_to_end(
                            &mut archive.by_index(index).unwrap(),
                            &mut bytes,
                        )
                        .unwrap();
                        assert_eq!(
                            bytes, original,
                            "only the selected internal page and its alias are packaged"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn only_rejected_links_warn_before_the_existing_no_page_images_error() {
    for dry_run in [true, false] {
        let parent = tempfile::tempdir().unwrap();
        if !links::supported(parent.path()) {
            return;
        }
        let root = parent.path().join("Book");
        fs::create_dir(&root).unwrap();
        links::file_link(Path::new("missing.png"), &root.join("p1.png")).unwrap();
        let destination = parent.path().join("output.epub");
        let mut command = Command::new(binary());
        command
            .arg(&root)
            .args(["--json-events", "--output"])
            .arg(&destination);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        let events = parse_events(&output);
        let warning = events
            .iter()
            .position(|event| event["code"] == "link_skipped")
            .unwrap();
        let error = events.last().unwrap();
        assert_eq!(error["type"], "error");
        assert_eq!(error["code"], "no_page_images");
        assert!(warning < events.len() - 1);
        assert!(!events
            .iter()
            .any(|event| event["type"] == "result" || event["stage"] == "process"));
        assert!(!destination.exists());
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 1);
    }
}

#[test]
fn an_internal_metadata_link_is_parsed_and_an_explicit_cbz_link_is_still_supported() {
    let parent = tempfile::tempdir().unwrap();
    if !links::supported(parent.path()) {
        return;
    }
    let root = fixture_folder();
    let metadata = root.path().join("metadata.txt");
    fs::write(
        &metadata,
        b"<ComicInfo><Series>Linked metadata title</Series></ComicInfo>",
    )
    .unwrap();
    links::file_link(&metadata, &root.path().join("ComicInfo.xml")).unwrap();
    let destination = parent.path().join("book.cbz");
    let output = Command::new(binary())
        .arg(root.path())
        .args([
            "--json-events",
            "--format",
            "cbz",
            "--noprocessing",
            "--output",
        ])
        .arg(&destination)
        .output()
        .unwrap();
    assert!(output.status.success());
    let events = parse_events(&output);
    assert_eq!(events.last().unwrap()["manga"], "Linked metadata title");
    assert_eq!(events.last().unwrap()["source_pages"], 4);
    assert!(!events.iter().any(|event| event["code"] == "link_skipped"));
    let selected = root.path().join("selected.cbz");
    links::file_link(&destination, &selected).unwrap();
    let output = Command::new(binary())
        .arg(&selected)
        .args(["--json-events", "--dry-run"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let events = parse_events(&output);
    assert_eq!(events.last().unwrap()["source_pages"], 4);
    assert!(!events.iter().any(|event| event["code"] == "link_skipped"));
}

#[cfg(windows)]
#[test]
fn windows_junctions_are_reported_by_the_real_binary_in_plan_and_conversion() {
    for dry_run in [true, false] {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("Book");
        fs::create_dir(&root).unwrap();
        write_png(&root.join("p1.png"), 32);
        let outside = parent.path().join("not-selected-private-directory");
        fs::create_dir(&outside).unwrap();
        write_png(&outside.join("private.png"), 200);
        links::junction(&root, &root.join("loop"));
        links::junction(&outside, &root.join("door"));
        let shortcut = parent.path().join("shortcut");
        links::junction(&root, &shortcut);
        let destination = parent.path().join("output.cbz");
        let mut command = Command::new(binary());
        command
            .arg(&shortcut)
            .args([
                "--json-events",
                "--format",
                "cbz",
                "--noprocessing",
                "--output",
            ])
            .arg(&destination);
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let warnings: Vec<_> = events
            .iter()
            .filter(|event| event["code"] == "link_skipped")
            .collect();
        assert_eq!(warnings.len(), 2);
        assert_eq!(
            Path::new(warnings[0]["path"].as_str().unwrap()),
            shortcut.join("door")
        );
        assert_eq!(
            Path::new(warnings[1]["path"].as_str().unwrap()),
            shortcut.join("loop")
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("not-selected-private-directory"));
        assert_eq!(events.last().unwrap()["source_pages"], 1);
        assert_eq!(events.last().unwrap()["written"], !dry_run);
        assert_eq!(destination.exists(), !dry_run);
    }
}
