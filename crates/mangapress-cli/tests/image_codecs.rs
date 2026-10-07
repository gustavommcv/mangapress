mod support;

use image::{ImageFormat, Rgb, RgbImage};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

fn write_cbz(folder: &Path, destination: &Path) {
    let mut archive = zip::ZipWriter::new(fs::File::create(destination).unwrap());
    let mut paths: Vec<_> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        archive
            .start_file(
                path.file_name().unwrap().to_str().unwrap(),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(&fs::read(path).unwrap()).unwrap();
    }
    archive.finish().unwrap();
}

#[test]
fn supported_page_formats_convert_from_folders_and_cbz_to_every_book_format() {
    let input = tempfile::tempdir().unwrap();
    write_png(&input.path().join("p01.png"), 89);
    let page = RgbImage::from_pixel(32, 48, Rgb([89, 89, 89]));
    for (name, format) in [
        ("p02.jpg", ImageFormat::Jpeg),
        ("p03.JPEG", ImageFormat::Jpeg),
        ("p04.gif", ImageFormat::Gif),
        ("p05.bmp", ImageFormat::Bmp),
        ("p06.webp", ImageFormat::WebP),
    ] {
        page.save_with_format(input.path().join(name), format)
            .unwrap();
    }
    let archive_dir = tempfile::tempdir().unwrap();
    let cbz = archive_dir.path().join("input.cbz");
    write_cbz(input.path(), &cbz);

    for source in [input.path(), cbz.as_path()] {
        for format in ["epub", "cbz", "pdf"] {
            for png in [false, true] {
                let output_dir = tempfile::tempdir().unwrap();
                let destination = output_dir.path().join(format!("book.{format}"));
                let mut command = Command::new(binary());
                command
                    .arg(source)
                    .args([
                        "--format",
                        format,
                        "--customwidth",
                        "32",
                        "--customheight",
                        "48",
                        "--cropping",
                        "disabled",
                        "--gamma",
                        "1",
                        "--noquantize",
                        "--json-events",
                        "--output",
                    ])
                    .arg(&destination);
                if png {
                    command.arg("--forcepng");
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "{source:?}, {format}, PNG={png}: {output:?}"
                );
                assert!(output.stderr.is_empty());
                let events = parse_events(&output);
                let result = events.last().unwrap();
                assert_eq!(result["type"], "result");
                assert_eq!(result["source_pages"], 6);
                assert_eq!(result["output_pages"], 6);
                assert_eq!(result["written"], true);
                assert_eq!(fs::read_dir(output_dir.path()).unwrap().count(), 1);
                if format == "pdf" {
                    assert!(fs::read(&destination).unwrap().starts_with(b"%PDF-"));
                    continue;
                }
                let mut book = zip::ZipArchive::new(fs::File::open(&destination).unwrap()).unwrap();
                let mut pages = 0;
                for index in 0..book.len() {
                    let mut entry = book.by_index(index).unwrap();
                    if !entry.name().ends_with(if png { ".png" } else { ".jpg" })
                        || entry.name().ends_with("cover.jpg")
                    {
                        continue;
                    }
                    let mut bytes = Vec::new();
                    entry.read_to_end(&mut bytes).unwrap();
                    let decoded = image::load_from_memory(&bytes).unwrap();
                    assert_eq!((decoded.width(), decoded.height()), (32, 48));
                    pages += 1;
                }
                assert_eq!(pages, 6);
            }
        }
    }
}

#[test]
fn an_unsupported_image_renamed_to_png_fails_cleanly_even_in_passthrough() {
    let input = fixture_folder();
    let pnm = b"P6\n1 1\n255\n\x01\x02\x03";
    fs::write(input.path().join("c001 - One/p0001.png"), pnm).unwrap();
    let archive_dir = tempfile::tempdir().unwrap();
    let cbz = archive_dir.path().join("input.cbz");
    write_cbz(&input.path().join("c001 - One"), &cbz);
    for source in [input.path(), cbz.as_path()] {
        for passthrough in [false, true] {
            let output_dir = tempfile::tempdir().unwrap();
            let destination = output_dir.path().join("book.epub");
            let mut command = Command::new(binary());
            command
                .arg(source)
                .args(["--json-events", "--output"])
                .arg(&destination);
            if passthrough {
                command.arg("--noprocessing");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            let events = parse_events(&output);
            let error = events.last().unwrap();
            assert_eq!(error["type"], "error");
            assert_eq!(error["code"], "page_processing_failed");
            assert_eq!(error["stage"], "process");
            assert!(
                error["diagnostic"].as_str().unwrap().contains("Pnm"),
                "{error}"
            );
            assert!(!events.iter().any(|event| event["type"] == "result"));
            assert_eq!(fs::read_dir(output_dir.path()).unwrap().count(), 0);
            assert_eq!(
                fs::read(input.path().join("c001 - One/p0001.png")).unwrap(),
                pnm
            );
        }
    }
}
