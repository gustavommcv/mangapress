// Each test binary uses some of the shared helpers, not all.
#[allow(dead_code)]
mod support;

use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use support::{binary, parse_events, write_png};

/// A `.cbz` holding these files, in this order; a name ending in `.png` gets a small page.
fn cbz(dir: &Path, files: &[&str]) -> PathBuf {
    let path = dir.join("Synthetic Book.cbz");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (index, name) in files.iter().enumerate() {
        archive
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        if name.ends_with(".png") {
            let page = dir.join(format!("page{index}.png"));
            write_png(&page, 40 + 20 * index as u8);
            archive.write_all(&std::fs::read(&page).unwrap()).unwrap();
        } else {
            archive.write_all(comic_info().as_bytes()).unwrap();
        }
    }
    archive.finish().unwrap();
    path
}

fn comic_info() -> &'static str {
    "<ComicInfo><Series>From the folder</Series><Writer>Ann</Writer></ComicInfo>"
}

fn convert(book: &Path, output: &Path, extra: &[&str]) -> Output {
    Command::new(binary())
        .arg(book)
        .args(["--profile", "KV", "--json-events", "--output"])
        .arg(output)
        .args(extra)
        .output()
        .expect("run mangapress")
}

fn contents_of(epub: &Path) -> String {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(epub).unwrap()).unwrap();
    let mut ncx = String::new();
    archive
        .by_name("OEBPS/toc.ncx")
        .unwrap()
        .read_to_string(&mut ncx)
        .unwrap();
    ncx
}

fn metadata_completed(events: &[Value]) -> &Value {
    events
        .iter()
        .find(|event| event["stage"] == "metadata" && event["state"] == "completed")
        .expect("metadata stage completed")
}

#[test]
fn pages_in_the_only_folder_are_the_books_and_its_comic_info_is_read() {
    let work = tempfile::tempdir().unwrap();
    let book = cbz(
        work.path(),
        &[
            "My Wrapper/001.png",
            "My Wrapper/002.png",
            "My Wrapper/ComicInfo.xml",
        ],
    );
    let epub = work.path().join("book.epub");

    let output = convert(&book, &epub, &[]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let completed = metadata_completed(&events);
    assert_eq!(completed["comic_info_found"], true);
    assert_eq!(completed["title"], "From the folder");
    assert_eq!(completed["author"], "Ann");
    // The pages lie in the book, so the contents list them under its title, not "My Wrapper".
    let ncx = contents_of(&epub);
    assert_eq!(ncx.matches("<navPoint").count(), 1);
    assert!(ncx.contains("<navLabel><text>From the folder</text></navLabel>"));
    assert!(!ncx.contains("My Wrapper"));
    // Nothing the input held was left unread.
    assert!(events
        .iter()
        .all(|event| event["code"] != "skipped_non_images"));
}

#[test]
fn a_volume_folder_that_holds_chapter_folders_still_names_the_volume_in_a_nested_contents() {
    // What Mangabind writes for one volume: the volume folder is the only entry of the archive.
    let work = tempfile::tempdir().unwrap();
    let book = cbz(
        work.path(),
        &[
            "v001 - Vol.01/c001 - One/p0001.png",
            "v001 - Vol.01/c001 - One/p0002.png",
            "v001 - Vol.01/c002 - Two/p0001.png",
        ],
    );
    let epub = work.path().join("book.epub");

    let output = convert(&book, &epub, &["--nested-toc"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ncx = contents_of(&epub);
    assert_eq!(ncx.matches("<navPoint").count(), 3, "{ncx}");
    for label in ["v001 - Vol.01", "c001 - One", "c002 - Two"] {
        assert!(
            ncx.contains(&format!("<navLabel><text>{label}</text></navLabel>")),
            "{label}\n{ncx}"
        );
    }
    let volume = ncx.find("v001 - Vol.01").unwrap();
    assert!(volume < ncx.find("c001 - One").unwrap());
}

#[test]
fn without_a_single_folder_a_comic_info_one_folder_down_is_ignored_and_counted() {
    let work = tempfile::tempdir().unwrap();
    let book = cbz(work.path(), &["A/001.png", "A/ComicInfo.xml", "B/001.png"]);

    let output = convert(&book, &work.path().join("planned.epub"), &["--dry-run"]);

    assert!(output.status.success());
    let events = parse_events(&output);
    assert_eq!(metadata_completed(&events)["comic_info_found"], false);
    let warning = events
        .iter()
        .find(|event| event["code"] == "skipped_non_images")
        .expect("the ignored file is counted");
    assert_eq!(warning["count"], 1);
}

#[test]
fn a_page_that_fails_in_the_only_folder_is_named_by_the_book_not_by_the_folder() {
    let work = tempfile::tempdir().unwrap();
    let broken = work.path().join("Synthetic Book.cbz");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&broken).unwrap());
    for (name, data) in [
        ("My Wrapper/001.png", b"not a PNG".as_slice()),
        ("My Wrapper/ComicInfo.xml", comic_info().as_bytes()),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(data).unwrap();
    }
    archive.finish().unwrap();

    let output = convert(&broken, &work.path().join("book.epub"), &[]);

    assert_eq!(output.status.code(), Some(1));
    let error = parse_events(&output).into_iter().last().unwrap();
    assert_eq!(error["code"], "page_processing_failed");
    assert_eq!(error["chapter"], "From the folder");
    assert!(error["diagnostic"].as_str().unwrap().contains("001.png"));
}
