//! The mangapress half of the parity check in `tools/parity/`: runs this
//! crate's real page pipeline over a list of images and writes every page
//! it produces as PNG, plus a `mangapress.json` describing them, so that
//! `tools/parity/parity.py` can set them beside what upstream KCC does with
//! the same images. Not part of the product; see `tools/parity/README.md`.
//!
//! ```text
//! parity_dump <out_dir> <list_file> <profile> [flags]     pipeline, one page per line of the list
//! parity_dump <out_dir> <list_file> <profile> --quantize-only   dither already-gray PNGs as they are
//! parity_dump <out_dir> <directory> <profile> --webtoon-dir     merge a folder into a strip and cut it
//! ```
//!
//! The first line of the list is the book's first page.

use mangapress_core::crop::inter_panel::InterPanelMode;
use mangapress_core::pipeline::{self, CroppingMode, OutputFormat, PipelineOptions, SplitterMode};
use mangapress_core::{fill_check::fill_check, Profile};
use std::io::BufRead;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: parity_dump <out_dir> <list_file|directory> <profile> [flags]");
        std::process::exit(2);
    }
    let (out_dir, list, profile) = (&args[1], &args[2], &args[3]);
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .find_map(|a| a.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')))
    };

    let number = |name: &str| value(name).map(|v| v.parse::<f32>().expect("a number"));

    let webtoon = flag("--webtoon");
    let options = PipelineOptions {
        profile: Profile::by_code(profile).expect("a known profile code"),
        width_override: None,
        height_override: None,
        // Upstream's webtoon mode forces these four; see the CLI.
        manga_style: flag("--manga") && !webtoon,
        cropping: match value("--crop") {
            Some("disabled") => CroppingMode::Disabled,
            Some("margins") => CroppingMode::Margins,
            _ => CroppingMode::MarginsAndPageNumbers,
        },
        cropping_power: number("--croppingpower").unwrap_or(1.0),
        cropping_minimum: number("--croppingminimum").unwrap_or(0.0),
        preserve_margin_percent: number("--preservemargin").unwrap_or(0.0),
        inter_panel_crop: match value("--ipc") {
            Some("horizontal") => InterPanelMode::Horizontal,
            Some("both") => InterPanelMode::Both,
            _ => InterPanelMode::Disabled,
        },
        splitter: match value("--splitter") {
            Some("rotate") => SplitterMode::Rotate,
            Some("both") => SplitterMode::Both,
            _ => SplitterMode::Split,
        },
        upscale: flag("--upscale") && !webtoon,
        stretch: flag("--stretch"),
        wallpaper: flag("--wallpaper"),
        white_borders: flag("--whiteborders") || webtoon,
        black_borders: flag("--blackborders") && !webtoon,
        rotate_right: flag("--rotateright"),
        no_rotate: flag("--norotate"),
        rotate_first: flag("--rotatefirst"),
        maximize_strips: flag("--maximizestrips"),
        color_autocontrast: flag("--colorautocontrast"),
        webtoon,
        force_color: flag("--forcecolor"),
        force_png_rgb: flag("--force-png-rgb"),
        png_legacy: flag("--pnglegacy"),
        no_quantize: flag("--noquantize"),
        no_processing: false,
        output_format: match value("--format") {
            Some("cbz") => OutputFormat::Cbz,
            Some("pdf") => OutputFormat::Pdf,
            _ => OutputFormat::Epub,
        },
        force_png: flag("--forcepng"),
        gamma: value("--gamma").map(|g| g.parse().expect("--gamma=<number>")),
        autolevel: flag("--autolevel"),
        noautocontrast: flag("--noautocontrast"),
        erase_rainbow: flag("--eraserainbow"),
        // As close to the pipeline's own pixels as a JPEG gets, so that what
        // is compared is the processing and not the codec.
        jpeg_quality: Some(100),
    };
    std::fs::create_dir_all(out_dir).expect("creating the output directory");

    if flag("--webtoon-dir") {
        let mut files: Vec<_> = std::fs::read_dir(list)
            .expect("reading the strip directory")
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        let images = files
            .iter()
            .map(|file| image::open(file).unwrap().to_rgb8())
            .collect();
        let strip = mangapress_core::webtoon::merge_strip(images).unwrap();
        strip.save(format!("{out_dir}/strip.png")).unwrap();
        let pages =
            mangapress_core::webtoon::split_strip(&strip, options.target_resolution()).unwrap();
        for (n, page) in pages.iter().enumerate() {
            page.save(format!("{out_dir}/page-{:04}.png", n + 1))
                .unwrap();
        }
        return;
    }

    let lines: Vec<String> = std::io::BufReader::new(std::fs::File::open(list).expect("the list"))
        .lines()
        .map(|line| line.unwrap().trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    if flag("--quantize-only") {
        for (n, path) in lines.iter().enumerate() {
            let gray = image::open(path).unwrap().to_luma8();
            mangapress_core::quantize::quantize_with_floyd_steinberg(
                &gray,
                options.profile.palette,
            )
            .save(format!("{out_dir}/{n:04}.png"))
            .unwrap();
        }
        return;
    }

    let mut pages = Vec::new();
    for (n, path) in lines.iter().enumerate() {
        let bytes = std::fs::read(path).expect("a readable source image");
        let background = fill_check(&mangapress_core::color::to_gray(
            &image::load_from_memory(&bytes).unwrap().to_rgb8(),
        ));
        let outputs = pipeline::process_page(&bytes, &options, n == 0).unwrap();
        let mut pieces = Vec::new();
        for (i, page) in outputs.iter().enumerate() {
            let decoded = image::load_from_memory(&page.bytes).unwrap();
            let name = format!("{n:04}_{i}.png");
            let path = format!("{out_dir}/{name}");
            if decoded.color().has_color() {
                decoded.to_rgb8().save(&path).unwrap();
            } else {
                decoded.to_luma8().save(&path).unwrap();
            }
            pieces.push(format!(
                r#"{{"file": "{name}", "size": [{}, {}], "role": "{:?}", "ext": "{}", "black_background": {}}}"#,
                decoded.width(),
                decoded.height(),
                page.role,
                page.extension,
                page.black_background
            ));
        }
        pages.push(format!(
            r#"{{"background": "{}", "pieces": [{}]}}"#,
            if background == mangapress_core::crop::Background::White {
                "white"
            } else {
                "black"
            },
            pieces.join(", ")
        ));
    }
    std::fs::write(
        format!("{out_dir}/mangapress.json"),
        format!("{{\"pages\": [\n{}\n]}}\n", pages.join(",\n")),
    )
    .unwrap();
}
