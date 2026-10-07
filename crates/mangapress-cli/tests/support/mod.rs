use image::{GrayImage, ImageFormat, Luma};
use serde_json::Value;
use std::path::Path;
use std::process::Output;

pub fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_mangapress")
}

pub fn parse_events(output: &Output) -> Vec<Value> {
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
        if matches!(event["type"].as_str(), Some("warning" | "error")) {
            let code = event["code"].as_str().expect("issues must carry a code");
            let reference = include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../docs/machine-protocol-v1.md"
            ));
            assert!(
                reference
                    .lines()
                    .any(|line| line.starts_with(&format!("| `{code}` |"))),
                "issue code {code} must be documented in the protocol reference"
            );
        }
    }
    events
}

pub fn write_png(path: &Path, gray: u8) {
    let image = GrayImage::from_pixel(32, 48, Luma([gray]));
    image
        .save_with_format(path, ImageFormat::Png)
        .expect("write test page");
}

pub fn fixture_folder() -> tempfile::TempDir {
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
