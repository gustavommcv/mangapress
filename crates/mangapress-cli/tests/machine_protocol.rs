mod support;

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

mod hostile_inputs {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/input_safety.rs"
    ));
}

#[test]
fn protocol_handshake_is_one_versioned_json_line() {
    let output = Command::new(binary())
        .arg("--protocol-version")
        .output()
        .expect("run mangapress");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let events = parse_events(&output);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "protocol");
    assert_eq!(
        events[0]["capabilities"],
        serde_json::json!(["events", "profiles", "nested_toc"])
    );
}

#[test]
fn profiles_are_machine_readable_in_declared_order() {
    let output = Command::new(binary())
        .args(["--list-profiles", "--json-events"])
        .output()
        .expect("run mangapress");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let events = parse_events(&output);
    let profiles: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] == "profile")
        .collect();
    assert_eq!(profiles.first().unwrap()["code"], "K1");
    assert_eq!(profiles.last().unwrap()["code"], "OTHER");
    assert_eq!(events.last().unwrap()["profile_count"], profiles.len());
}

#[test]
fn dry_run_emits_stable_plan_events_without_human_output() {
    let fixture = fixture_folder();
    std::fs::write(fixture.path().join("notes.txt"), b"not an image").expect("write warning input");
    let output_path = tempfile::tempdir()
        .expect("create output parent")
        .path()
        .join("planned.epub");

    let output = Command::new(binary())
        .arg(fixture.path())
        .args(["--profile", "KV", "--dry-run", "--json-events", "--output"])
        .arg(&output_path)
        .output()
        .expect("run mangapress");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let events = parse_events(&output);
    let stages: Vec<(&str, &str)> = events
        .iter()
        .filter(|event| event["type"] == "stage")
        .map(|event| {
            (
                event["stage"].as_str().unwrap(),
                event["state"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        stages,
        vec![
            ("inspect", "started"),
            ("metadata", "started"),
            ("metadata", "completed"),
            ("inspect", "completed"),
            ("plan", "started"),
            ("plan", "completed"),
        ]
    );
    let warning = events
        .iter()
        .find(|event| event["type"] == "warning")
        .expect("non-image warning");
    assert_eq!(warning["code"], "skipped_non_images");
    let result = events.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["chapters"], 2);
    assert_eq!(result["source_pages"], 4);
    assert_eq!(result["written"], false);
    assert!(!output_path.exists());
}

#[test]
fn execution_streams_ordered_chapter_and_page_progress_then_writes() {
    let fixture = fixture_folder();
    let output_dir = tempfile::tempdir().expect("create output folder");
    let output_path = output_dir.path().join("converted.epub");

    let output = Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            "KV",
            "--cropping",
            "disabled",
            "--noautocontrast",
            "--json-events",
            "--output",
        ])
        .arg(&output_path)
        .output()
        .expect("run mangapress");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());

    let events = parse_events(&output);
    let page_events: Vec<&Value> = events
        .iter()
        .filter(|event| event["type"] == "page")
        .collect();
    assert_eq!(page_events.len(), 4);
    for (index, event) in page_events.iter().enumerate() {
        assert_eq!(event["completed"], index + 1);
        assert_eq!(event["total"], 4);
        assert!(event["chapter"].is_string());
        assert!(event["page"].is_number());
    }
    let chapter_states: Vec<&str> = events
        .iter()
        .filter(|event| event["type"] == "chapter")
        .map(|event| event["state"].as_str().unwrap())
        .collect();
    assert_eq!(
        chapter_states,
        ["started", "completed", "started", "completed"]
    );
    let result = events.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["written"], true);
    assert_eq!(result["source_pages"], 4);
    assert_eq!(result["output_pages"], 4);
    assert!(output_path.exists());
    assert!(std::fs::metadata(output_path).unwrap().len() > 1024);
}

#[test]
fn a_kobo_book_is_named_kepub_and_still_reported_as_epub() {
    let input = fixture_folder();
    let output_dir = tempfile::tempdir().expect("create output folder");
    for dry_run in [true, false] {
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args(["--profile", "KoC", "--format", "epub", "--output"])
            .arg(output_dir.path())
            .arg("--json-events");
        if dry_run {
            command.arg("--dry-run");
        }
        let output = command.output().expect("run mangapress");
        assert!(output.status.success());

        let events = parse_events(&output);
        // Every event that names a format names the format, not the file's
        // extension: consumers validate it against epub, cbz and pdf.
        for event in events.iter().filter(|event| !event["format"].is_null()) {
            assert_eq!(event["format"], "epub", "{event}");
        }
        let result = events.last().unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["format"], "epub");
        let path = result["output_path"].as_str().unwrap();
        assert!(path.ends_with(".kepub.epub"), "{path}");
        assert_eq!(PathBuf::from(path).exists(), !dry_run);
    }

    // With --nokepub the name is a plain .epub, and the format the same.
    let output = Command::new(binary())
        .arg(input.path())
        .args([
            "--profile",
            "KoC",
            "--format",
            "epub",
            "--nokepub",
            "--output",
        ])
        .arg(output_dir.path())
        .args(["--json-events", "--dry-run"])
        .output()
        .expect("run mangapress");
    let events = parse_events(&output);
    let result = events.last().unwrap();
    assert_eq!(result["format"], "epub");
    let path = result["output_path"].as_str().unwrap();
    assert!(
        path.ends_with(".epub") && !path.ends_with(".kepub.epub"),
        "{path}"
    );
}

#[test]
fn a_page_failure_has_chapter_page_stage_and_actionable_message() {
    let fixture = tempfile::tempdir().expect("create fixture folder");
    let chapter = fixture.path().join("c001 - Broken");
    std::fs::create_dir_all(&chapter).expect("create chapter");
    std::fs::write(chapter.join("p0001.png"), b"not a PNG").expect("write broken page");

    let output = Command::new(binary())
        .arg(fixture.path())
        .args(["--profile", "KV", "--json-events"])
        .output()
        .expect("run mangapress");
    assert_eq!(output.status.code(), Some(1));

    let events = parse_events(&output);
    let error = events.last().unwrap();
    assert_eq!(error["type"], "error");
    assert_eq!(error["severity"], "error");
    assert_eq!(error["code"], "page_processing_failed");
    assert_eq!(error["stage"], "process");
    assert_eq!(error["chapter"], "c001 - Broken");
    assert_eq!(error["page"], 1);
    assert!(error["diagnostic"].as_str().unwrap().contains("p0001.png"));
    assert!(error["diagnostic"]
        .as_str()
        .unwrap()
        .contains("processing page 1"));
    assert_eq!(
        error["message"],
        "Couldn't process page 1 in chapter 'c001 - Broken'."
    );
}

#[test]
fn nested_toc_with_a_non_epub_format_is_refused() {
    let fixture = fixture_folder();
    let output_dir = tempfile::tempdir().expect("create output folder");

    let output = Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            "KV",
            "--format",
            "cbz",
            "--nested-toc",
            "--json-events",
            "--output",
        ])
        .arg(output_dir.path().join("converted.cbz"))
        .output()
        .expect("run mangapress");
    assert_eq!(output.status.code(), Some(1));

    let events = parse_events(&output);
    let error = events.last().unwrap();
    assert_eq!(error["type"], "error");
    assert_eq!(error["code"], "nested_toc_unsupported_format");
    assert_eq!(error["stage"], "configuration");
    assert_eq!(error["recoverable"], true);
}

#[test]
fn nested_toc_epub_produces_a_two_level_table_of_contents() {
    // What Mangabind's -combine mode actually produces: a volume directory
    // wrapping ordinary chapter directories - see
    // docs/adr/0012-nested-toc-for-combined-volumes.md.
    let fixture = tempfile::tempdir().expect("create fixture folder");
    for (volume, chapter, pages) in [
        ("v001 - Vol.01", "c001 - One", [32u8, 64u8]),
        ("v001 - Vol.01", "c002 - Two", [96, 128]),
        ("v002 - Vol.02", "c001 - Three", [160, 192]),
    ] {
        let chapter_path = fixture.path().join(volume).join(chapter);
        std::fs::create_dir_all(&chapter_path).expect("create chapter");
        for (index, gray) in pages.into_iter().enumerate() {
            write_png(&chapter_path.join(format!("p{:04}.png", index + 1)), gray);
        }
    }

    let output_dir = tempfile::tempdir().expect("create output folder");
    let output_path = output_dir.path().join("converted.epub");

    let output = Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            "KV",
            "--cropping",
            "disabled",
            "--noautocontrast",
            "--nested-toc",
            "--output",
        ])
        .arg(&output_path)
        .output()
        .expect("run mangapress");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let epub_bytes = std::fs::read(&output_path).expect("read produced epub");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(epub_bytes)).expect("open epub as zip");
    let mut ncx = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("OEBPS/toc.ncx").unwrap(), &mut ncx)
        .expect("read toc.ncx");

    let doc = roxmltree::Document::parse(&ncx).expect("toc.ncx must be well-formed XML");
    let nav_map = doc
        .descendants()
        .find(|n| n.has_tag_name("navMap"))
        .expect("navMap element");
    let top_level_nav_points: Vec<_> = nav_map
        .children()
        .filter(|n| n.has_tag_name("navPoint"))
        .collect();
    assert_eq!(
        top_level_nav_points.len(),
        2,
        "two volumes at the top level, not three flat chapters"
    );

    let volume_titles: Vec<String> = top_level_nav_points
        .iter()
        .map(|v| {
            v.descendants()
                .find(|n| n.has_tag_name("text"))
                .and_then(|t| t.text())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(volume_titles, vec!["v001 - Vol.01", "v002 - Vol.02"]);

    let volume1_children: Vec<_> = top_level_nav_points[0]
        .children()
        .filter(|n| n.has_tag_name("navPoint"))
        .collect();
    assert_eq!(
        volume1_children.len(),
        2,
        "volume 1's two chapters must be nested directly inside its own navPoint"
    );
    let volume2_children: Vec<_> = top_level_nav_points[1]
        .children()
        .filter(|n| n.has_tag_name("navPoint"))
        .collect();
    assert_eq!(volume2_children.len(), 1);
}

#[test]
fn usage_failures_emit_a_structured_error_before_clap_diagnostics() {
    let output = Command::new(binary())
        .arg("--json-events")
        .output()
        .expect("run mangapress");
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());

    let events = parse_events(&output);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "error");
    assert_eq!(events[0]["code"], "invalid_arguments");
    assert_eq!(events[0]["stage"], "configuration");
    assert_eq!(events[0]["recoverable"], true);
}

#[test]
fn real_mangabind_fixture_emits_parseable_conversion_when_available() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let real_fixture = std::fs::read_dir(&workspace)
        .expect("read workspace")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|extension| extension.to_str()) == Some("cbz"));
    let Some(real_fixture) = real_fixture else {
        eprintln!("no local real Mangabind CBZ; skipping according to tests/fixtures/README.md");
        return;
    };

    let output_directory = tempfile::tempdir().expect("create real-fixture output folder");
    let output_path = output_directory.path().join("real-fixture.epub");
    let output = Command::new(binary())
        .arg(real_fixture)
        .args(["--profile", "KV", "--json-events", "--output"])
        .arg(&output_path)
        .output()
        .expect("run mangapress against real fixture");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let result = events.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["dry_run"], false);
    assert!(result["chapters"].as_u64().unwrap() > 0);
    assert!(result["source_pages"].as_u64().unwrap() > 0);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "page")
            .count() as u64,
        result["source_pages"].as_u64().unwrap()
    );
    assert_eq!(result["written"], true);
    assert!(output_path.exists());
    assert_eq!(
        std::fs::metadata(output_path).unwrap().len(),
        result["bytes"].as_u64().unwrap()
    );
}

#[test]
fn a_forged_zip64_size_fails_cleanly_in_human_and_machine_modes() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("forged.cbz");
    std::fs::write(
        &input,
        hostile_inputs::zip_with_declared_size(&[("p1.png", b"tiny")], 0, 1 << 62),
    )
    .unwrap();
    let destination = temp.path().join("existing.cbz");
    std::fs::write(&destination, b"previous book").unwrap();
    for machine in [false, true] {
        let mut command = Command::new(binary());
        command
            .arg(&input)
            .args(["--format", "cbz", "--output"])
            .arg(&destination);
        if machine {
            command.arg("--json-events");
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "no panic or abnormal process exit"
        );
        if machine {
            let events = parse_events(&output);
            let error = events.last().unwrap();
            assert_eq!(error["type"], "error");
            assert_eq!(error["code"], "input_read_failed");
            assert_eq!(error["stage"], "inspect");
            assert!(error["diagnostic"]
                .as_str()
                .unwrap()
                .contains("268435456-byte limit"));
            assert!(!events.iter().any(|event| event["type"] == "result"));
        } else {
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("268435456-byte limit"));
        }
        assert_eq!(std::fs::read(&destination).unwrap(), b"previous book");
    }
}

#[test]
fn discarded_archive_payloads_are_never_loaded_and_metadata_survives_conversion() {
    let temp = tempfile::tempdir().unwrap();
    let page_path = temp.path().join("source.png");
    write_png(&page_path, 89);
    let page = std::fs::read(page_path).unwrap();
    let xml = b"<ComicInfo><Series>Metadata title</Series></ComicInfo>";
    let bytes = hostile_inputs::zip_with_declared_size(
        &[
            ("ComicInfo.xml", xml),
            ("notes.txt", b"tiny"),
            ("c001/p1.PNG", &page),
            ("c001/ComicInfo.xml", b"ignored"),
            ("__MACOSX/._p1.png", b"ignored"),
        ],
        1,
        1 << 62,
    );
    let input = temp.path().join("source.cbz");
    std::fs::write(&input, bytes).unwrap();
    for dry_run in [true, false] {
        let destination = temp.path().join("book.cbz");
        let mut command = Command::new(binary());
        command
            .arg(&input)
            .args([
                "--json-events",
                "--format",
                "cbz",
                "--keepcomicinfo",
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
        let warning = events
            .iter()
            .find(|event| event["code"] == "skipped_non_images")
            .unwrap();
        assert_eq!(warning["count"], 3);
        let result = events.last().unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["manga"], "Metadata title");
        assert_eq!(result["source_pages"], 1);
        assert_eq!(result["written"], !dry_run);
        if !dry_run {
            let mut archive =
                zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
            let mut actual_xml = Vec::new();
            std::io::Read::read_to_end(
                &mut archive.by_name("ComicInfo.xml").unwrap(),
                &mut actual_xml,
            )
            .unwrap();
            assert_eq!(actual_xml, xml);
        } else {
            assert!(!destination.exists());
        }
    }
}

#[test]
fn image_limits_apply_to_processing_passthrough_webtoon_spreads_and_covers() {
    for (flags, expected_code, stage, custom_cover) in [
        (vec![], "page_processing_failed", "process", false),
        (
            vec!["--noprocessing"],
            "page_processing_failed",
            "process",
            false,
        ),
        (vec!["--webtoon"], "webtoon_split_failed", "process", false),
        (vec!["--spreads"], "spread_join_failed", "inspect", false),
        (vec!["--cover"], "cover_build_failed", "package", true),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input");
        std::fs::create_dir(&input).unwrap();
        let oversized = hostile_inputs::oversized_bmp();
        if custom_cover {
            write_png(&input.join("p1.png"), 89);
        } else {
            std::fs::write(input.join("p1.bmp"), &oversized).unwrap();
        }
        write_png(&input.join("p2.png"), 89);
        let labels = temp.path().join("spreads.json");
        std::fs::write(&labels, br#"{"spreads": [0]}"#).unwrap();
        let cover = temp.path().join("cover.bmp");
        std::fs::write(&cover, oversized).unwrap();
        let destination = temp.path().join("book.epub");
        let mut command = Command::new(binary());
        command
            .arg(&input)
            .args(["--json-events", "--output"])
            .arg(&destination)
            .args(&flags);
        if flags.contains(&"--spreads") {
            command.arg(&labels);
        }
        if custom_cover {
            command.arg(&cover);
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(1),
            "{flags:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let error = events.last().unwrap();
        assert_eq!(error["type"], "error", "{flags:?}: {error}");
        assert_eq!(error["code"], expected_code, "{flags:?}");
        assert_eq!(error["stage"], stage, "{flags:?}");
        let pixel_limit = mangapress_core::input::MAX_IMAGE_PIXELS;
        assert!(
            error["diagnostic"]
                .as_str()
                .unwrap()
                .contains(&format!("{pixel_limit}-pixel limit")),
            "{flags:?}: {error}"
        );
        assert!(!destination.exists());
        assert!(!events.iter().any(|event| event["type"] == "result"));
    }
}

#[test]
fn early_filtering_keeps_empty_and_no_image_diagnostics_distinct() {
    for (file, code, skipped) in [
        (None, "input_empty", 0),
        (
            Some(("notes.txt", b"notes".as_slice())),
            "no_page_images",
            1,
        ),
        (
            Some(("ComicInfo.xml", b"<ComicInfo/>".as_slice())),
            "no_page_images",
            0,
        ),
    ] {
        let input = tempfile::tempdir().unwrap();
        if let Some((name, bytes)) = file {
            std::fs::write(input.path().join(name), bytes).unwrap();
        }
        let output = Command::new(binary())
            .arg(input.path())
            .args(["--dry-run", "--json-events"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        let events = parse_events(&output);
        assert_eq!(events.last().unwrap()["code"], code);
        let warning = events
            .iter()
            .find(|event| event["code"] == "skipped_non_images");
        if skipped == 0 {
            assert!(warning.is_none());
        } else {
            assert_eq!(warning.unwrap()["count"], skipped);
        }
    }
}
