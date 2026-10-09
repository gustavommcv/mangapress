// Each test binary uses some of the shared helpers, not all.
#[allow(dead_code)]
mod support;

use image::{ImageFormat, RgbImage};
use std::path::Path;
use std::process::{Command, Output};
use support::{binary, parse_events, write_png};

fn noisy_png(path: &Path, cut_tenths: Option<usize>) {
    let image = RgbImage::from_fn(120, 160, |x, y| {
        let noise = (x * 7 + y * 13 + x * y) as u8;
        image::Rgb([noise, noise.wrapping_mul(3), y as u8])
    });
    image
        .save_with_format(path, ImageFormat::Png)
        .expect("write test page");
    if let Some(tenths) = cut_tenths {
        let bytes = std::fs::read(path).unwrap();
        std::fs::write(path, &bytes[..bytes.len() * tenths / 10]).unwrap();
    }
}

fn convert(input: &Path, extra: &[&str]) -> Output {
    let work = tempfile::tempdir().unwrap();
    Command::new(binary())
        .arg(input)
        .args(["--profile", "K11", "--json-events", "--output"])
        .arg(work.path().join("book.epub"))
        .args(extra)
        .output()
        .expect("run mangapress")
}

fn book_with_a_cut_page() -> tempfile::TempDir {
    let book = tempfile::tempdir().unwrap();
    let chapter = book.path().join("c001 - One");
    std::fs::create_dir_all(&chapter).unwrap();
    write_png(&chapter.join("p0001.png"), 90);
    noisy_png(&chapter.join("p0002.png"), Some(6));
    write_png(&chapter.join("p0003.png"), 150);
    book
}

#[test]
fn a_png_cut_short_makes_a_book_and_a_warning_that_names_it() {
    let book = book_with_a_cut_page();
    let output = convert(book.path(), &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let warnings: Vec<_> = events
        .iter()
        .filter(|event| event["code"] == "page_truncated")
        .collect();
    assert_eq!(warnings.len(), 1, "{events:#?}");
    let warning = warnings[0];
    assert_eq!(warning["type"], "warning");
    assert_eq!(warning["stage"], "process");
    assert_eq!(warning["severity"], "warning");
    assert_eq!(warning["recoverable"], true);
    assert_eq!(warning["chapter"], "c001 - One");
    assert_eq!(warning["page"], 2);
    assert!(warning["path"].as_str().unwrap().ends_with("p0002.png"));
    assert!(warning["message"].as_str().unwrap().contains("p0002.png"));

    let result = events.last().unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["output_pages"], 3);
}

#[test]
fn the_terminal_says_which_page_ended_early() {
    let book = book_with_a_cut_page();
    let work = tempfile::tempdir().unwrap();
    let output = Command::new(binary())
        .arg(book.path())
        .args(["--profile", "K11", "--quiet", "--output"])
        .arg(work.path().join("book.epub"))
        .output()
        .expect("run mangapress");

    assert!(output.status.success());
    let said = String::from_utf8_lossy(&output.stderr);
    assert!(
        said.contains("warning: Page 2 of chapter 'c001 - One'")
            && said.contains("p0002.png")
            && said.contains("the rest of the page is blank"),
        "{said}"
    );
}

#[test]
fn whole_pages_make_no_warning() {
    let book = tempfile::tempdir().unwrap();
    let chapter = book.path().join("c001 - One");
    std::fs::create_dir_all(&chapter).unwrap();
    noisy_png(&chapter.join("p0001.png"), None);
    write_png(&chapter.join("p0002.png"), 150);

    let events = parse_events(&convert(book.path(), &[]));

    assert!(events.iter().all(|event| event["code"] != "page_truncated"));
}

#[test]
fn a_png_that_ends_before_its_header_does_is_still_an_error() {
    let book = tempfile::tempdir().unwrap();
    let chapter = book.path().join("c001 - One");
    std::fs::create_dir_all(&chapter).unwrap();
    write_png(&chapter.join("p0001.png"), 90);
    let bytes = std::fs::read(chapter.join("p0001.png")).unwrap();
    std::fs::write(chapter.join("p0002.png"), &bytes[..16]).unwrap();

    let output = convert(book.path(), &[]);

    assert!(!output.status.success());
    let events = parse_events(&output);
    assert_eq!(events.last().unwrap()["code"], "page_processing_failed");
}
