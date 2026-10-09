mod support;

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};
use support::{binary, fixture_folder, parse_events};

fn convert(folder: &Path, output: &Path, extra: &[&str]) -> Output {
    Command::new(binary())
        .arg(folder)
        .args(["--profile", "KV", "--output"])
        .arg(output)
        .args(extra)
        .output()
        .expect("run mangapress")
}

fn utf16_with_mark(text: &str, little_endian: bool) -> Vec<u8> {
    let units = text.encode_utf16();
    if little_endian {
        [0xFF, 0xFE]
            .into_iter()
            .chain(units.flat_map(u16::to_le_bytes))
            .collect()
    } else {
        [0xFE, 0xFF]
            .into_iter()
            .chain(units.flat_map(u16::to_be_bytes))
            .collect()
    }
}

fn metadata_completed(events: &[Value]) -> &Value {
    events
        .iter()
        .find(|event| {
            event["type"] == "stage"
                && event["stage"] == "metadata"
                && event["state"] == "completed"
        })
        .expect("metadata stage completed")
}

#[test]
fn a_comic_info_that_is_not_well_formed_is_a_warning_and_the_book_is_still_made() {
    let fixture = fixture_folder();
    std::fs::write(
        fixture.path().join("ComicInfo.xml"),
        b"<ComicInfo><Series>Cut short</Series>",
    )
    .unwrap();
    let work = tempfile::tempdir().unwrap();
    let book = work.path().join("book.epub");

    let output = convert(fixture.path(), &book, &["--json-events"]);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(book.is_file(), "the book is written");
    let events = parse_events(&output);
    let warning = events
        .iter()
        .find(|event| event["code"] == "comic_info_unreadable")
        .expect("a warning names the problem");
    assert_eq!(warning["type"], "warning");
    assert_eq!(warning["stage"], "metadata");
    assert_eq!(warning["recoverable"], true);
    assert!(warning["message"]
        .as_str()
        .unwrap()
        .contains("ComicInfo.xml"));
    assert_eq!(metadata_completed(&events)["comic_info_found"], false);
    assert_eq!(events.last().unwrap()["written"], true);
    assert!(events.iter().all(|event| event["type"] != "error"));
}

#[test]
fn the_same_problem_is_one_line_on_the_terminal_without_machine_events() {
    let fixture = fixture_folder();
    std::fs::write(fixture.path().join("ComicInfo.xml"), b"not xml at all").unwrap();
    let work = tempfile::tempdir().unwrap();
    let book = work.path().join("book.epub");

    let output = convert(fixture.path(), &book, &[]);

    assert!(output.status.success());
    assert!(book.is_file());
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        said.matches("warning: ComicInfo.xml could not be read and was ignored")
            .count(),
        1,
        "{said}"
    );
}

#[test]
fn a_comic_info_in_utf16_is_read_whichever_way_its_bytes_run() {
    for little_endian in [true, false] {
        let fixture = fixture_folder();
        let xml = "<?xml version=\"1.0\" encoding=\"utf-16\"?><ComicInfo><Series>Sixteen</Series><Writer>Ann</Writer></ComicInfo>";
        std::fs::write(
            fixture.path().join("ComicInfo.xml"),
            utf16_with_mark(xml, little_endian),
        )
        .unwrap();
        let work = tempfile::tempdir().unwrap();

        let output = convert(
            fixture.path(),
            &work.path().join("planned.epub"),
            &["--dry-run", "--json-events"],
        );

        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let completed = metadata_completed(&events);
        assert_eq!(completed["comic_info_found"], true);
        assert_eq!(completed["title"], "Sixteen");
        assert_eq!(completed["author"], "Ann");
        assert!(events
            .iter()
            .all(|event| event["code"] != "comic_info_unreadable"));
    }
}

#[test]
fn a_readable_comic_info_gives_no_warning() {
    let fixture = fixture_folder();
    std::fs::write(
        fixture.path().join("ComicInfo.xml"),
        "<ComicInfo><Series>Fine</Series><Writer>Ann</Writer></ComicInfo>",
    )
    .unwrap();
    let work = tempfile::tempdir().unwrap();

    let output = convert(
        fixture.path(),
        &work.path().join("planned.epub"),
        &["--dry-run", "--json-events"],
    );

    let events = parse_events(&output);
    assert!(events
        .iter()
        .all(|event| event["code"] != "comic_info_unreadable"));
    assert_eq!(metadata_completed(&events)["title"], "Fine");
}
