mod support;

use std::process::{Command, Output, Stdio};
use support::{binary, fixture_folder, parse_events};

#[test]
fn help_is_grouped_and_shows_usage_examples_exit_codes_and_support() {
    for arguments in [
        vec!["--help"],
        vec!["-h"],
        vec!["--profile", "BOGUS", "--json-events", "--help"],
    ] {
        let output = Command::new(binary()).args(arguments).output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("mangapress [OPTIONS] <INPUT>"));
        assert!(!help.contains("mangapress [OPTIONS] [INPUT]"));
        for section in [
            "Conversion",
            "Reporting",
            "Page layout",
            "Cropping and sizing",
            "Image quality",
            "Covers and chapters",
            "Book metadata",
            "Examples",
            "Exit codes",
        ] {
            assert!(
                help.contains(&format!("{section}:")),
                "missing section {section}"
            );
        }
        assert!(help.contains("mangapress \"Volume 1.cbz\" --profile K11 --format epub"));
        assert!(help.contains("mangapress --list-profiles [--json-events]"));
        assert!(help.contains("mangapress --protocol-version"));
        assert!(help.contains(concat!(env!("CARGO_PKG_REPOSITORY"), "#readme")));
        assert!(help.contains(concat!(env!("CARGO_PKG_REPOSITORY"), "/issues")));
        assert!(
            !help.contains('\u{1b}'),
            "piped help should not contain terminal escapes"
        );
    }
}

#[test]
fn version_uses_the_package_version_and_short_circuits_conversion() {
    let output = Command::new(binary())
        .args(["--profile", "BOGUS", "--json-events", "--version"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("mangapress ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn usage_and_runtime_errors_keep_their_exit_codes_and_streams() {
    let directory = tempfile::tempdir().unwrap();
    for (arguments, code) in [
        (vec![], 2),
        (vec!["--unknown-option"], 2),
        (vec!["missing.cbz", "--jpeg-quality", "0"], 2),
        (vec!["missing.cbz"], 1),
    ] {
        let output = Command::new(binary())
            .current_dir(directory.path())
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn human_dry_run_reports_the_resolved_format_without_creating_output() {
    let input = fixture_folder();
    let destination = tempfile::tempdir().unwrap();
    for (profile, format) in [("K11", "epub"), ("KDX", "cbz"), ("Rmk2", "pdf")] {
        let path = destination
            .path()
            .join("not-created")
            .join(format!("book.{format}"));
        let output = Command::new(binary())
            .arg(input.path())
            .args([
                "--dry-run",
                "--quiet",
                "--upscale",
                "--profile",
                profile,
                "--output",
            ])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let summary = String::from_utf8(output.stdout).unwrap();
        assert!(summary
            .lines()
            .any(|line| line == format!("format: {format}")));
        assert!(summary.contains("chapters: 2, pages: 4"));
        assert!(summary.contains(&format!("would write: {}", path.display())));
        assert!(!path.parent().unwrap().exists());
    }
}

#[test]
fn page_errors_name_the_real_second_image_in_folders_and_archives() {
    let folder = fixture_folder();
    let valid = folder.path().join("c001 - One/p0001.png");
    let broken = folder.path().join("c001 - One/scan-20.png");
    std::fs::rename(folder.path().join("c001 - One/p0002.png"), &broken).unwrap();
    std::fs::write(&broken, b"not a PNG").unwrap();
    let archive_root = tempfile::tempdir().unwrap();
    let archive = archive_root.path().join("Broken pages.cbz");
    let bytes = mangapress_core::archive::cbz::write_zip(
        &[
            (
                "c001 - One/p0001.png".to_owned(),
                std::fs::read(valid).unwrap(),
            ),
            (
                "c001 - One/scan-20.png".to_owned(),
                std::fs::read(broken).unwrap(),
            ),
            // A second chapter, as in the folder: one folder alone would be read as the book itself.
            (
                "c002 - Two/p0001.png".to_owned(),
                std::fs::read(folder.path().join("c002 - Two/p0001.png")).unwrap(),
            ),
        ],
        true,
    )
    .unwrap();
    std::fs::write(&archive, bytes).unwrap();
    let destination = tempfile::tempdir().unwrap();

    for input in [folder.path(), archive.as_path()] {
        for quiet in [false, true] {
            for machine in [false, true] {
                let path = destination.path().join("book.cbz");
                let mut command = Command::new(binary());
                command
                    .arg(input)
                    .args([
                        "--format",
                        "cbz",
                        "--customwidth",
                        "32",
                        "--customheight",
                        "48",
                        "--cropping",
                        "disabled",
                        "--noautocontrast",
                        "--output",
                    ])
                    .arg(&path);
                if quiet {
                    command.arg("--quiet");
                }
                if machine {
                    command.arg("--json-events");
                }
                let output = command.output().unwrap();
                assert_eq!(output.status.code(), Some(1));
                let diagnostic = String::from_utf8_lossy(&output.stderr);
                assert!(
                    diagnostic.contains("processing page 2 in chapter 'c001 - One'"),
                    "{diagnostic}"
                );
                assert!(diagnostic.contains("scan-20.png"), "{diagnostic}");
                assert!(!diagnostic.contains("panicked"));
                if machine {
                    let events = parse_events(&output);
                    let error = events.last().unwrap();
                    assert_eq!(error["code"], "page_processing_failed");
                    assert_eq!(error["page"], 2);
                    assert_eq!(error["chapter"], "c001 - One");
                    assert!(error["diagnostic"]
                        .as_str()
                        .unwrap()
                        .contains("scan-20.png"));
                    assert_eq!(std::path::Path::new(error["path"].as_str().unwrap()), input);
                    assert!(!events.iter().any(|event| event["type"] == "result"));
                } else {
                    assert!(output.stdout.is_empty());
                }
                assert!(!path.exists());
                assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
            }
        }
    }
}

fn large_title_fixture() -> tempfile::TempDir {
    let input = fixture_folder();
    // Exceed typical pipe buffers so closing the reader interrupts the report even if writes start first.
    let xml = format!(
        "<ComicInfo><Series>{}</Series></ComicInfo>",
        "A".repeat(128 * 1024)
    );
    std::fs::write(input.path().join("ComicInfo.xml"), xml).unwrap();
    input
}

fn close_stdout(mut command: Command) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    child.wait_with_output().unwrap()
}

#[test]
fn human_dry_run_stops_quietly_when_the_reader_closes_stdout() {
    let input = large_title_fixture();
    let destination = tempfile::tempdir().unwrap();
    let path = destination.path().join("book.epub");
    let mut command = Command::new(binary());
    command
        .arg(input.path())
        .args(["--dry-run", "--quiet", "--upscale", "--output"])
        .arg(&path);
    let output = close_stdout(command);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(!path.exists());
}

#[test]
fn a_closed_json_stream_remains_a_protocol_failure_not_human_success() {
    let input = large_title_fixture();
    let mut command = Command::new(binary());
    command
        .arg(input.path())
        .args(["--dry-run", "--json-events"]);
    let output = close_stdout(command);
    assert_eq!(output.status.code(), Some(1));
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("writing JSON event"), "{diagnostic}");
    assert!(!diagnostic.contains("panicked"));
}

#[test]
fn human_profile_listing_remains_a_plain_table_without_input() {
    let output = Command::new(binary())
        .args(["--list-profiles", "--quiet"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let table = String::from_utf8(output.stdout).unwrap();
    assert!(table.lines().next().unwrap().starts_with("K1 "));
    assert!(table.lines().last().unwrap().starts_with("OTHER "));
    assert!(table
        .lines()
        .all(|line| line.split_whitespace().last().unwrap().contains('x')));
}
