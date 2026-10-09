mod support;

use serde_json::Value;
use std::process::{Command, Output};
use support::{binary, fixture_folder, parse_events};

fn plan(profile: &str, extra: &[&str]) -> Output {
    let fixture = fixture_folder();
    let work = tempfile::tempdir().unwrap();
    Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            profile,
            "--dry-run",
            "--json-events",
            "--output",
        ])
        .arg(work.path().join("planned.epub"))
        .args(extra)
        .output()
        .expect("run mangapress")
}

fn gray_levels(output: &Output) -> u64 {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(output);
    let plan: &Value = events
        .iter()
        .find(|event| {
            event["type"] == "stage" && event["stage"] == "plan" && event["state"] == "completed"
        })
        .expect("a plan event");
    plan["gray_levels"]
        .as_u64()
        .expect("gray levels in the plan")
}

#[test]
fn a_device_keeps_its_own_gray_levels_at_its_own_size() {
    assert_eq!(gray_levels(&plan("K1", &[])), 4);
    assert_eq!(gray_levels(&plan("K2", &[])), 15);
    assert_eq!(gray_levels(&plan("K11", &[])), 16);
}

#[test]
fn a_custom_width_or_height_gives_sixteen_gray_levels_whatever_the_device() {
    for extra in [
        &["--customwidth", "800"][..],
        &["--customheight", "1200"][..],
        &["--customwidth", "800", "--customheight", "1200"][..],
    ] {
        assert_eq!(gray_levels(&plan("K1", extra)), 16, "K1 {extra:?}");
        assert_eq!(gray_levels(&plan("K2", extra)), 16, "K2 {extra:?}");
    }
}

#[test]
fn the_terminal_banner_says_the_levels_of_the_pages_it_will_make() {
    let fixture = fixture_folder();
    let work = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            "K1",
            "--customwidth",
            "800",
            "--dry-run",
            "--output",
        ])
        .arg(work.path().join("planned.epub"))
        .output()
        .expect("run mangapress");

    assert!(output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(said.contains("16 gray levels"), "{said}");
    assert!(!said.contains("4 gray levels"), "{said}");
}
