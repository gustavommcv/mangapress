mod support;

use std::io::Read;
use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn centered_pages_keep_valid_spine_properties_across_profiles_and_codecs() {
    let fixture = fixture_folder();
    image::GrayImage::from_pixel(64, 48, image::Luma([32]))
        .save(fixture.path().join("c001 - One/p0001.png"))
        .unwrap();
    for profile in ["K11", "KS3", "KoLC", "RmkPP", "OTHER"] {
        for png in [false, true] {
            for one_page_landscape in [false, true] {
                let output_dir = tempfile::tempdir().unwrap();
                let destination = output_dir.path().join("book.epub");
                let mut command = Command::new(binary());
                command.arg(fixture.path()).args([
                    "--profile",
                    profile,
                    "--format",
                    "epub",
                    "--cropping",
                    "disabled",
                    "--splitter",
                    "rotate",
                    "--json-events",
                    "--output",
                ]);
                command.arg(&destination);
                if profile == "OTHER" {
                    command.args(["--customwidth", "127", "--customheight", "193"]);
                }
                if png {
                    command.arg("--forcepng");
                }
                if one_page_landscape {
                    command.arg("--onepagelandscape");
                }
                let output = command.output().unwrap();
                assert!(output.status.success(), "{profile}, PNG={png}: {output:?}");
                assert!(output.stderr.is_empty());
                let events = parse_events(&output);
                assert_eq!(events.last().unwrap()["output_pages"], 4);
                let mut book =
                    zip::ZipArchive::new(std::fs::File::open(&destination).unwrap()).unwrap();
                let mut opf = String::new();
                book.by_name("OEBPS/content.opf")
                    .unwrap()
                    .read_to_string(&mut opf)
                    .unwrap();
                let document = roxmltree::Document::parse(&opf).unwrap();
                let items: Vec<_> = document
                    .descendants()
                    .filter(|node| node.has_tag_name("itemref"))
                    .collect();
                assert_eq!(items.len(), 4);
                let sides = if one_page_landscape {
                    ["center", "center", "center", "center"]
                } else {
                    ["center", "left", "right", "left"]
                };
                let kindle = matches!(profile, "K11" | "KS3");
                for (index, (item, side)) in items.iter().zip(sides).enumerate() {
                    let prefix = if kindle && side != "center" {
                        ""
                    } else {
                        "rendition:"
                    };
                    assert_eq!(
                        item.attribute("properties"),
                        Some(format!("{prefix}page-spread-{side}").as_str()),
                        "{profile}, PNG={png}, one_page={one_page_landscape}"
                    );
                    assert_eq!(
                        item.attribute("idref"),
                        Some(format!("page{}", index + 1).as_str())
                    );
                    assert_eq!(item.attribute("linear"), kindle.then_some("yes"));
                }
            }
        }
    }
}
