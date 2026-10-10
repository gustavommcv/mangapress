mod support;

use std::io::{Cursor, Read};
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

fn member(bytes: &[u8], path: &str) -> Vec<u8> {
    let mut book = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut result = Vec::new();
    book.by_name(path)
        .unwrap()
        .read_to_end(&mut result)
        .unwrap();
    result
}

#[test]
fn native_scribe_and_custom_epubs_keep_package_metadata_and_result_counts() {
    let fixture = fixture_folder();
    std::fs::write(fixture.path().join("ComicInfo.xml"), b"<ComicInfo><Series>Series &amp; Co</Series><Number>2</Number><Writer>Writer, Artist</Writer><Summary>A &lt;summary&gt;</Summary></ComicInfo>").unwrap();
    for (profile, flags, resolution, series) in [
        ("KS3", vec![], Some("1920x2648"), false),
        ("KoLC", vec![], None, true),
        (
            "K11",
            vec!["--customwidth", "32", "--customheight", "48"],
            None,
            false,
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("book.epub");
        let output = Command::new(binary())
            .arg(fixture.path())
            .args([
                "--profile",
                profile,
                "--format",
                "epub",
                "--noprocessing",
                "--language",
                "pt",
                "--json-events",
                "--output",
            ])
            .arg(&destination)
            .args(&flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let events = parse_events(&output);
        let bytes = std::fs::read(&destination).unwrap();
        let package = String::from_utf8(member(&bytes, "OEBPS/content.opf")).unwrap();
        let opf = roxmltree::Document::parse(&package).unwrap();
        assert_eq!(
            opf.descendants()
                .find(|n| n.attribute("name") == Some("original-resolution"))
                .and_then(|n| n.attribute("content")),
            resolution
        );
        assert_eq!(
            opf.descendants()
                .any(|n| n.attribute("property") == Some("belongs-to-collection")),
            series
        );
        assert_eq!(
            opf.descendants()
                .filter(|n| n.has_tag_name("creator"))
                .map(|n| n.text().unwrap())
                .collect::<Vec<_>>(),
            ["Artist", "Writer"]
        );
        assert_eq!(
            opf.descendants()
                .find(|n| n.has_tag_name("language"))
                .unwrap()
                .text(),
            Some("pt")
        );
        assert_eq!(
            opf.descendants()
                .find(|n| n.has_tag_name("description"))
                .unwrap()
                .text(),
            Some("A <summary>")
        );
        let result = events.last().unwrap();
        assert_eq!(result["source_pages"], 4);
        assert_eq!(result["output_pages"], 4);
        assert_eq!(result["bytes"], bytes.len());
        assert_eq!(
            result["manga"].as_str(),
            opf.descendants()
                .find(|n| n.has_tag_name("title"))
                .unwrap()
                .text()
        );
        let package_events = events
            .iter()
            .filter(|e| e["type"] == "stage" && e["stage"] == "package")
            .collect::<Vec<_>>();
        assert_eq!(package_events.len(), 2);
        assert_eq!(package_events[0]["state"], "started");
        assert_eq!(package_events[1]["state"], "completed");
        assert_eq!(package_events[1]["bytes"], bytes.len());
    }
}

#[test]
fn cbz_keeps_original_xml_and_smart_default_cover_without_changing_source_pages() {
    let fixture = fixture_folder();
    let first_path = fixture.path().join("c001 - One/p0001.png");
    image::RgbImage::from_fn(96, 64, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 3) as u8, (x + y) as u8])
    })
    .save(&first_path)
    .unwrap();
    let original = std::fs::read(&first_path).unwrap();
    let text = "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\r\n<ComicInfo><Title>Book &amp; title</Title></ComicInfo>\r\n";
    let mut original_xml = vec![0xff, 0xfe];
    original_xml.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(fixture.path().join("ComicInfo.xml"), &original_xml).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("book.cbz");
    let output = Command::new(binary())
        .arg(fixture.path())
        .args([
            "--profile",
            "K11",
            "--format",
            "cbz",
            "--noprocessing",
            "--keepcomicinfo",
            "--smartcovercrop",
            "--json-events",
            "--output",
        ])
        .arg(&destination)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = parse_events(&output);
    let bytes = std::fs::read(destination).unwrap();
    let mut book = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let names = (0..book.len())
        .map(|i| book.by_index(i).unwrap().name().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(&names[..2], ["##cover.jpg", "ComicInfo.xml"]);
    assert_eq!(member(&bytes, "ComicInfo.xml"), original_xml);
    let cover = image::load_from_memory(&member(&bytes, "##cover.jpg")).unwrap();
    // The existing smart-cover builder excludes the jacket's spine as well
    // as its back; this 1.5:1 source produces a 45-pixel front cover.
    assert_eq!((cover.width(), cover.height()), (45, 64));
    assert_eq!(
        member(
            &bytes,
            names.iter().find(|n| n.ends_with("p0001.png")).unwrap()
        ),
        original
    );
    assert_eq!(names.iter().filter(|n| n.ends_with(".png")).count(), 4);
    assert_eq!(events.last().unwrap()["source_pages"], 4);
    assert_eq!(events.last().unwrap()["output_pages"], 4);
}

#[test]
fn broken_cover_stops_package_and_cleans_staging_but_pdf_omits_it() {
    let fixture = fixture_folder();
    write_png(&fixture.path().join("c002 - Two/p0002.png"), 130);
    let cover_directory = tempfile::tempdir().unwrap();
    let cover = cover_directory.path().join("broken.png");
    std::fs::write(&cover, b"broken image").unwrap();
    for format in ["epub", "cbz", "pdf"] {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join(format!("book.{format}"));
        let output = Command::new(binary())
            .arg(fixture.path())
            .args([
                "--profile",
                "K11",
                "--format",
                format,
                "--noprocessing",
                "--json-events",
                "--cover",
            ])
            .arg(&cover)
            .arg("--output")
            .arg(&destination)
            .output()
            .unwrap();
        let events = parse_events(&output);
        assert!(events
            .iter()
            .any(|e| e["stage"] == "process" && e["state"] == "completed"));
        if format == "pdf" {
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            assert_eq!(events.last().unwrap()["type"], "result");
            assert_eq!(events.last().unwrap()["source_pages"], 4);
            assert_eq!(events.last().unwrap()["output_pages"], 4);
            assert_eq!(
                events.last().unwrap()["bytes"],
                std::fs::metadata(destination).unwrap().len()
            );
        } else {
            assert!(!output.status.success());
            let issue = events.last().unwrap();
            assert_eq!(issue["code"], "cover_build_failed");
            assert_eq!(issue["stage"], "package");
            assert_eq!(issue["recoverable"], true);
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e["type"] == "stage" && e["stage"] == "package")
                    .count(),
                1
            );
            assert!(!events
                .iter()
                .any(|e| e["stage"] == "write" || e["type"] == "result"));
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }
}
