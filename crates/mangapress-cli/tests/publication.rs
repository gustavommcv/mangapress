mod support;

use std::io::Write;
use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn complete_books_keep_human_quiet_and_machine_publication_reports() {
    let fixture = fixture_folder();
    for format in ["epub", "cbz", "pdf"] {
        for mode in ["human", "quiet", "machine", "machine-quiet"] {
            let directory = tempfile::tempdir().unwrap();
            let destination = directory.path().join(format!("book.{format}"));
            let mut command = Command::new(binary());
            command
                .arg(fixture.path())
                .args([
                    "--profile",
                    "K11",
                    "--format",
                    format,
                    "--noprocessing",
                    "--title",
                    "Named book",
                    "--author",
                    "Named author",
                    "--customwidth",
                    "32",
                    "--customheight",
                    "48",
                    "--output",
                ])
                .arg(&destination);
            if mode.contains("quiet") {
                command.arg("--quiet");
            }
            if mode.contains("machine") {
                command.arg("--json-events");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let length = std::fs::metadata(&destination).unwrap().len();
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
            if mode.contains("machine") {
                assert!(output.stderr.is_empty());
                let events = parse_events(&output);
                let tail = &events[events.len() - 3..];
                assert_eq!(tail[0]["stage"], "write");
                assert_eq!(tail[0]["state"], "started");
                assert_eq!(tail[1]["stage"], "write");
                assert_eq!(tail[1]["state"], "completed");
                assert_eq!(tail[2]["type"], "result");
                assert_eq!(tail[0]["path"], destination.to_str().unwrap());
                assert_eq!(tail[1]["path"], tail[2]["output_path"]);
                assert_eq!(tail[1]["bytes"], length);
                assert_eq!(tail[2]["bytes"], length);
                assert_eq!(tail[2]["manga"], "Named book");
                assert_eq!(tail[2]["author"], "Named author");
                assert_eq!(tail[2]["format"], format);
                assert_eq!(tail[2]["profile"], "K11");
                assert_eq!(tail[2]["width"], 32);
                assert_eq!(tail[2]["height"], 48);
                assert_eq!(tail[2]["chapters"], 2);
                assert_eq!(tail[2]["source_pages"], 4);
                assert_eq!(tail[2]["output_pages"], 4);
                assert_eq!(tail[2]["written"], true);
                assert_eq!(tail[2]["dry_run"], false);
            } else {
                assert!(output.stdout.is_empty());
                let stderr = String::from_utf8(output.stderr).unwrap();
                let expected = format!("wrote {} ({length} bytes)\n", destination.display());
                assert_eq!(stderr.contains(&expected), mode == "human");
            }
        }
    }
}

#[test]
fn result_page_counts_keep_spread_expansion_and_webtoon_cutting_distinct() {
    for webtoon in [false, true] {
        let fixture = tempfile::tempdir().unwrap();
        let image = if webtoon {
            image::GrayImage::from_fn(320, 1600, |x, y| {
                image::Luma([if (32..288).contains(&x) && (100..1500).contains(&y) {
                    if (x / 16) % 2 == 0 {
                        140
                    } else {
                        60
                    }
                } else {
                    255
                }])
            })
        } else {
            image::GrayImage::from_fn(96, 64, |x, _| image::Luma([if x < 48 { 60 } else { 170 }]))
        };
        image.save(fixture.path().join("01.png")).unwrap();
        if !webtoon {
            support::write_png(&fixture.path().join("02.png"), 110);
        }
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("book.cbz");
        let mut command = Command::new(binary());
        command
            .arg(fixture.path())
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
            .arg(&destination);
        if webtoon {
            command.arg("--webtoon");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let events = parse_events(&output);
        let result = events.last().unwrap();
        let plan = events
            .iter()
            .find(|e| e["stage"] == "plan" && e["state"] == "completed")
            .unwrap();
        let process = events
            .iter()
            .find(|e| e["type"] == "stage" && e["stage"] == "process" && e["state"] == "completed")
            .unwrap();
        assert_eq!(result["source_pages"], process["source_pages"]);
        assert_eq!(result["output_pages"], process["output_pages"]);
        assert_eq!(result["chapters"], 1);
        assert_eq!(
            result["bytes"],
            std::fs::metadata(destination).unwrap().len()
        );
        if webtoon {
            assert_eq!(plan["pages"], 1);
            assert!(result["source_pages"].as_u64().unwrap() > 1);
        } else {
            assert_eq!(plan["pages"], 2);
            assert_eq!(result["source_pages"], 2);
            assert_eq!(result["output_pages"], 3);
        }
    }
}

#[test]
fn reported_destinations_keep_numbered_compound_and_input_collision_names() {
    let fixture = fixture_folder();
    for case in ["numbered", "compound", "input-alias"] {
        let directory = tempfile::tempdir().unwrap();
        let profile = if case == "compound" { "KoLC" } else { "K11" };
        let format = if case == "input-alias" { "cbz" } else { "epub" };
        let mut source = fixture.path().to_path_buf();
        let destination = directory.path().join(if case == "compound" {
            "book.kepub.epub"
        } else if case == "input-alias" {
            "Source.with.dots.cbz"
        } else {
            "book.epub"
        });
        let expected = directory.path().join(if case == "compound" {
            "book (mangapress).kepub.epub"
        } else if case == "input-alias" {
            "Source.with.dots (mangapress).cbz"
        } else {
            "book (mangapress 2).epub"
        });
        let first_alternate = directory.path().join("book (mangapress).epub");
        if case == "input-alias" {
            let mut archive = zip::ZipWriter::new(std::fs::File::create(&destination).unwrap());
            archive
                .start_file("01.png", zip::write::SimpleFileOptions::default())
                .unwrap();
            archive
                .write_all(&std::fs::read(fixture.path().join("c001 - One/p0001.png")).unwrap())
                .unwrap();
            archive.finish().unwrap();
            source = destination.clone();
        } else {
            std::fs::write(&destination, b"keep existing book").unwrap();
            if case == "numbered" {
                std::fs::write(&first_alternate, b"keep first alternate").unwrap();
            }
        }
        let original = std::fs::read(&destination).unwrap();
        let output = Command::new(binary())
            .arg(&source)
            .args([
                "--profile",
                profile,
                "--format",
                format,
                "--noprocessing",
                "--title",
                "Metadata does not name the file",
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
        assert!(output.stderr.is_empty());
        assert_eq!(std::fs::read(&destination).unwrap(), original);
        if case == "numbered" {
            assert_eq!(
                std::fs::read(first_alternate).unwrap(),
                b"keep first alternate"
            );
        }
        let events = parse_events(&output);
        let result = events.last().unwrap();
        assert_eq!(result["output_path"], expected.to_str().unwrap());
        assert_eq!(result["format"], format);
        assert_eq!(result["bytes"], std::fs::metadata(&expected).unwrap().len());
        assert_eq!(result["manga"], "Metadata does not name the file");
        assert!(events.iter().any(|e| e["code"]
            == if case == "input-alias" {
                "output_collision"
            } else {
                "output_exists"
            }));
        assert!(std::fs::read_dir(directory.path())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".mangapress-")));
    }
}
