use image::{GrayImage, ImageFormat, Luma};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mangapress")
}

fn parse_events(output: &Output) -> Vec<Value> {
    let stdout = String::from_utf8(output.stdout.clone()).expect("machine stdout must be UTF-8");
    let events: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("every stdout line must be one JSON object"))
        .collect();
    assert!(!events.is_empty(), "machine output must contain events");
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["protocol_version"], 1);
        assert_eq!(event["tool"], "mangapress");
        assert_eq!(event["sequence"], index + 1);
        assert!(event["tool_version"].is_string());
        assert!(event["type"].is_string());
    }
    events
}

fn write_png(path: &Path, gray: u8) {
    let image = GrayImage::from_pixel(32, 48, Luma([gray]));
    image
        .save_with_format(path, ImageFormat::Png)
        .expect("write test page");
}

fn fixture_folder() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("create fixture folder");
    for (chapter, pages) in [("c001 - One", [32, 64]), ("c002 - Two", [96, 128])] {
        let chapter_path = temp.path().join(chapter);
        std::fs::create_dir_all(&chapter_path).expect("create chapter");
        for (index, gray) in pages.into_iter().enumerate() {
            write_png(&chapter_path.join(format!("p{:04}.png", index + 1)), gray);
        }
    }
    temp
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
        serde_json::json!(["events", "profiles"])
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
    assert_eq!(
        error["message"],
        "Couldn't process page 1 in chapter 'c001 - Broken'."
    );
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
