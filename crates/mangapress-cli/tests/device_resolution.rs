mod support;

use std::io::Read;
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

#[test]
fn kindle_dx_plan_and_packaged_pages_use_the_same_format_specific_target() {
    let fixture = fixture_folder();
    for (args, expected) in [
        (vec![], (824, 1200)),
        (vec!["--format", "cbz"], (824, 1200)),
        (vec!["--customwidth", "824"], (824, 1000)),
        (vec!["--customheight", "1000"], (824, 1000)),
        (
            vec!["--customwidth", "40", "--customheight", "60"],
            (40, 60),
        ),
    ] {
        let output_dir = tempfile::tempdir().unwrap();
        let destination = output_dir.path().join("book.cbz");
        for dry_run in [true, false] {
            let mut command = Command::new(binary());
            command
                .arg(fixture.path())
                .args([
                    "--profile",
                    "KDX",
                    "--stretch",
                    "--cropping",
                    "disabled",
                    "--forcepng",
                    "--noquantize",
                    "--json-events",
                    "--output",
                ])
                .arg(&destination)
                .args(&args);
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.stderr.is_empty());
            let events = parse_events(&output);
            let plan = events
                .iter()
                .find(|e| e["type"] == "stage" && e["stage"] == "plan" && e["state"] == "completed")
                .expect("completed plan must report the processing target");
            for event in [plan, events.last().unwrap()] {
                assert_eq!(event["width"], expected.0);
                assert_eq!(event["height"], expected.1);
                assert_eq!(event["format"], "cbz");
            }
            assert_eq!(events.last().unwrap()["written"], !dry_run);
            if dry_run {
                assert!(!destination.exists());
                assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 0);
                continue;
            }
            let mut book =
                zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
            let mut page_count = 0;
            for index in 0..book.len() {
                let mut entry = book.by_index(index).unwrap();
                if entry.name().ends_with(".png") {
                    let mut bytes = Vec::new();
                    entry.read_to_end(&mut bytes).unwrap();
                    let page = image::load_from_memory(&bytes).unwrap();
                    assert_eq!((page.width(), page.height()), expected);
                    page_count += 1;
                }
            }
            assert_eq!(page_count, 4);
        }
    }
}

#[test]
fn kindle_dx_human_plan_reports_cbz_target_but_profile_listing_keeps_builtin_size() {
    let fixture = fixture_folder();
    let destination = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .arg(fixture.path())
        .args(["--profile", "KDX", "--dry-run", "--output"])
        .arg(destination.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("device: Kindle DX/DXG (824x1200)"));

    let output = Command::new(binary())
        .args(["--list-profiles", "--json-events"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let events = parse_events(&output);
    let profile = events
        .iter()
        .find(|e| e["type"] == "profile" && e["code"] == "KDX")
        .unwrap();
    assert_eq!(profile["width"], 824);
    assert_eq!(profile["height"], 1000);
}

#[test]
fn kindle_dx_epub_and_pdf_plans_keep_the_builtin_resolution() {
    let fixture = fixture_folder();
    for format in ["epub", "pdf"] {
        let destination = tempfile::tempdir().unwrap();
        let output = Command::new(binary())
            .arg(fixture.path())
            .args([
                "--profile",
                "KDX",
                "--format",
                format,
                "--dry-run",
                "--json-events",
                "--output",
            ])
            .arg(destination.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let events = parse_events(&output);
        let result = events.last().unwrap();
        assert_eq!(result["format"], format);
        assert_eq!(result["width"], 824);
        assert_eq!(result["height"], 1000);
    }
}

#[test]
fn scribe_epub_plan_pages_and_metadata_agree_on_the_width_cap() {
    let fixture = tempfile::tempdir().unwrap();
    write_png(&fixture.path().join("page.png"), 96);
    for profile in ["KS3", "KSCS"] {
        for (args, expected, kindle_metadata) in [
            (vec!["--format", "epub"], (1920, 2648), true),
            (
                vec!["--format", "epub", "--customwidth", "1986"],
                (1986, 2648),
                false,
            ),
            (
                vec!["--format", "epub", "--customheight", "2648"],
                (1986, 2648),
                false,
            ),
        ] {
            let output_dir = tempfile::tempdir().unwrap();
            let destination = output_dir.path().join("book.epub");
            for dry_run in [true, false] {
                let mut command = Command::new(binary());
                command
                    .arg(fixture.path())
                    .args([
                        "--profile",
                        profile,
                        "--stretch",
                        "--cropping",
                        "disabled",
                        "--forcepng",
                        "--noquantize",
                        "--json-events",
                        "--output",
                    ])
                    .arg(&destination)
                    .args(&args);
                if dry_run {
                    command.arg("--dry-run");
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(output.stderr.is_empty());
                let events = parse_events(&output);
                let plan = events
                    .iter()
                    .find(|e| {
                        e["type"] == "stage" && e["stage"] == "plan" && e["state"] == "completed"
                    })
                    .unwrap();
                for event in [plan, events.last().unwrap()] {
                    assert_eq!(event["width"], expected.0);
                    assert_eq!(event["height"], expected.1);
                    assert_eq!(event["format"], "epub");
                }
                assert_eq!(events.last().unwrap()["written"], !dry_run);
                if dry_run {
                    assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 0);
                    continue;
                }
                let mut book =
                    zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
                let mut opf = String::new();
                book.by_name("OEBPS/content.opf")
                    .unwrap()
                    .read_to_string(&mut opf)
                    .unwrap();
                let package = roxmltree::Document::parse(&opf).unwrap();
                let resolution = package
                    .descendants()
                    .find(|node| {
                        node.has_tag_name("meta")
                            && node.attribute("name") == Some("original-resolution")
                    })
                    .and_then(|node| node.attribute("content"));
                assert_eq!(resolution, kindle_metadata.then_some("1920x2648"));
                let mut page_count = 0;
                for index in 0..book.len() {
                    let mut entry = book.by_index(index).unwrap();
                    if entry.name().ends_with(".png") {
                        let mut bytes = Vec::new();
                        entry.read_to_end(&mut bytes).unwrap();
                        let page = image::load_from_memory(&bytes).unwrap();
                        assert_eq!((page.width(), page.height()), expected);
                        page_count += 1;
                    }
                }
                assert_eq!(page_count, 1);
            }
        }
    }
}

#[test]
fn scribe_cbz_and_pdf_plans_and_profile_listing_keep_the_full_resolution() {
    let fixture = fixture_folder();
    let listing = Command::new(binary())
        .args(["--list-profiles", "--json-events"])
        .output()
        .unwrap();
    assert!(listing.status.success());
    let profiles = parse_events(&listing);
    for code in ["KS3", "KSCS"] {
        let profile = profiles
            .iter()
            .find(|e| e["type"] == "profile" && e["code"] == code)
            .unwrap();
        assert_eq!(profile["width"], 1986);
        assert_eq!(profile["height"], 2648);
        for format in ["cbz", "pdf"] {
            let output_dir = tempfile::tempdir().unwrap();
            let output = Command::new(binary())
                .arg(fixture.path())
                .args([
                    "--profile",
                    code,
                    "--format",
                    format,
                    "--dry-run",
                    "--json-events",
                    "--output",
                ])
                .arg(output_dir.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            let events = parse_events(&output);
            let result = events.last().unwrap();
            assert_eq!(result["format"], format);
            assert_eq!(result["width"], 1986);
            assert_eq!(result["height"], 2648);
        }
    }
}
