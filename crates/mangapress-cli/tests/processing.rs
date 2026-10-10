mod support;

use image::{GrayImage, ImageFormat, Luma};
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

fn command(input: &std::path::Path, output: &std::path::Path) -> Command {
    let mut command = Command::new(binary());
    command
        .arg(input)
        .args([
            "--profile",
            "K11",
            "--format",
            "cbz",
            "--customwidth",
            "32",
            "--customheight",
            "48",
            "--cropping",
            "disabled",
            "--noautocontrast",
            "--forcepng",
            "--json-events",
            "--output",
        ])
        .arg(output);
    command
}

#[test]
fn root_chapter_progress_counts_sources_not_split_outputs_and_preserves_book_title() {
    let input = tempfile::tempdir().unwrap();
    GrayImage::from_pixel(96, 64, Luma([90]))
        .save_with_format(input.path().join("01.png"), ImageFormat::Png)
        .unwrap();
    write_png(&input.path().join("02.png"), 130);
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("book.cbz");
    let output = command(input.path(), &destination)
        .args(["--title", "Named book"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let pages: Vec<_> = events.iter().filter(|e| e["type"] == "page").collect();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0]["chapter"], "Named book");
    assert_eq!(pages[0]["completed"], 1);
    assert_eq!(pages[1]["completed"], 2);
    assert!(pages.iter().all(|e| e["total"] == 2));
    let chapter = events
        .iter()
        .find(|e| e["type"] == "chapter" && e["state"] == "completed")
        .unwrap();
    assert_eq!(chapter["source_pages"], 2);
    assert_eq!(chapter["output_pages"], 3);
    let result = events.last().unwrap();
    assert_eq!(result["source_pages"], 2);
    assert_eq!(result["output_pages"], 3);
    assert!(destination.exists());
}

#[test]
fn webtoon_plan_counts_original_strips_then_execution_counts_generated_pages() {
    let input = fixture_folder();
    for name in ["c001 - One", "c002 - Two"] {
        for number in 1..=2 {
            GrayImage::from_fn(320, 1600, |x, y| {
                Luma([if (32..288).contains(&x) && (100..1500).contains(&y) {
                    if (x / 16) % 2 == 0 {
                        140
                    } else {
                        60
                    }
                } else {
                    255
                }])
            })
            .save_with_format(
                input.path().join(name).join(format!("p{number:04}.png")),
                ImageFormat::Png,
            )
            .unwrap();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("book.cbz");
    let output = command(input.path(), &destination)
        .args(["--webtoon", "--noprocessing", "--manga-style", "--upscale"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let plan = events
        .iter()
        .find(|e| e["stage"] == "plan" && e["state"] == "completed")
        .unwrap();
    assert_eq!(plan["pages"], 4);
    let start = events
        .iter()
        .find(|e| e["stage"] == "process" && e["state"] == "started")
        .unwrap();
    let pages: Vec<_> = events.iter().filter(|e| e["type"] == "page").collect();
    assert!(pages.len() > 4);
    assert_eq!(start["pages"], pages.len());
    for (index, event) in pages.iter().enumerate() {
        assert_eq!(event["completed"], index + 1);
        assert_eq!(event["total"], pages.len());
    }
    let chapter_sources: u64 = events
        .iter()
        .filter(|e| e["type"] == "chapter" && e["state"] == "started")
        .map(|e| e["source_pages"].as_u64().unwrap())
        .sum();
    assert_eq!(chapter_sources, pages.len() as u64);
    assert_eq!(events.last().unwrap()["source_pages"], pages.len());
    assert_eq!(events.last().unwrap()["output_pages"], pages.len());
}

#[test]
fn failure_in_a_later_chapter_keeps_prior_progress_and_discards_staged_output() {
    let input = fixture_folder();
    std::fs::write(
        input.path().join("c002 - Two").join("p0001.png"),
        b"bad png",
    )
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("book.cbz");
    let output = command(input.path(), &destination).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let events = parse_events(&output);
    let first = events
        .iter()
        .find(|e| e["type"] == "chapter" && e["state"] == "completed")
        .unwrap();
    assert_eq!(first["chapter"], "c001 - One");
    assert_eq!(first["completed"], 2);
    let error = events.last().unwrap();
    assert_eq!(error["code"], "page_processing_failed");
    assert_eq!(error["chapter"], "c002 - Two");
    assert_eq!(error["page"], 1);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e["stage"].as_str(), Some("package" | "write"))
                || e["type"] == "result")
    );
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert!(!destination.exists());
}
