use super::*;

struct FailingWriter {
    kind: std::io::ErrorKind,
    fail_on_flush: bool,
}

impl std::io::Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.fail_on_flush {
            Ok(bytes.len())
        } else {
            Err(self.kind.into())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_on_flush {
            Err(self.kind.into())
        } else {
            Ok(())
        }
    }
}

#[test]
fn human_reports_ignore_only_broken_pipe_including_flush_failures() {
    for kind in [
        std::io::ErrorKind::BrokenPipe,
        std::io::ErrorKind::PermissionDenied,
    ] {
        for fail_on_flush in [false, true] {
            let mut writer = FailingWriter {
                kind,
                fail_on_flush,
            };
            let result = write_human_report(&mut writer, |out| writeln!(out, "report"));
            if kind == std::io::ErrorKind::BrokenPipe {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.unwrap_err().kind(), kind);
            }
        }
    }
    let mut output = Vec::new();
    write_human_report(&mut output, |out| writeln!(out, "report")).unwrap();
    assert_eq!(output, b"report\n");
}

#[test]
fn automatic_format_follows_the_device_family() {
    let format = |code: &str| automatic_format(Profile::by_code(code).unwrap());
    for code in ["K1", "K2", "K34", "KDX"] {
        assert_eq!(format(code), Format::Cbz, "{code}");
    }
    assert_eq!(format("K11"), Format::Epub);
    assert_eq!(format("KoC"), Format::Epub);
    assert_eq!(format("Rmk2"), Format::Pdf);
    assert_eq!(format("OTHER"), Format::Epub);
}

#[test]
fn utc_timestamp_formats_known_instants() {
    let at = |seconds: u64| {
        utc_timestamp(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
    };
    assert_eq!(at(0), "1970-01-01T00:00:00Z");
    // A leap day, the day after it, and a year boundary.
    assert_eq!(at(1_709_164_800), "2024-02-29T00:00:00Z");
    assert_eq!(at(1_709_251_199), "2024-02-29T23:59:59Z");
    assert_eq!(at(1_709_251_200), "2024-03-01T00:00:00Z");
    assert_eq!(at(1_790_998_496), "2026-10-03T03:34:56Z");
    assert_eq!(at(4_102_444_799), "2099-12-31T23:59:59Z");
}

/// A series folder: the named books (a name ending in `/` is a folder)
/// and the named images in its `Covers` folder.
fn series(books: &[&str], covers: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for book in books {
        match book.strip_suffix('/') {
            Some(folder) => std::fs::create_dir(dir.path().join(folder)).unwrap(),
            None => std::fs::write(dir.path().join(book), b"x").unwrap(),
        }
    }
    std::fs::create_dir(dir.path().join(COVERS_FOLDER)).unwrap();
    for cover in covers {
        std::fs::write(dir.path().join(COVERS_FOLDER).join(cover), b"x").unwrap();
    }
    dir
}

fn cover_for(dir: &tempfile::TempDir, book: &str) -> Option<String> {
    let found = cover_by_convention(&dir.path().join(book))?;
    assert_eq!(found.parent().unwrap(), dir.path().join(COVERS_FOLDER));
    Some(found.file_name().unwrap().to_string_lossy().into_owned())
}

#[test]
fn spread_labels_are_looked_for_under_the_inputs_own_name_plus_json() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Vol 1.cbz");
    let folder = dir.path().join("Vol 2");
    std::fs::write(&file, b"x").unwrap();
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(spread_labels_beside(&file), None);

    std::fs::write(dir.path().join("Vol 1.cbz.json"), b"{}").unwrap();
    std::fs::write(dir.path().join("Vol 2.json"), b"{}").unwrap();
    assert_eq!(
        spread_labels_beside(&file),
        Some(dir.path().join("Vol 1.cbz.json"))
    );
    assert_eq!(
        spread_labels_beside(&folder),
        Some(dir.path().join("Vol 2.json"))
    );
    // A trailing separator on the folder changes nothing.
    let with_separator = PathBuf::from(format!("{}/", folder.display()));
    assert_eq!(
        spread_labels_beside(&with_separator),
        Some(dir.path().join("Vol 2.json"))
    );
}

#[test]
fn spread_labels_are_a_list_of_page_positions_under_spreads() {
    let dir = tempfile::tempdir().unwrap();
    let read = |text: &str| {
        let path = dir.path().join("labels.json");
        std::fs::write(&path, text).unwrap();
        read_spread_labels(&path)
    };
    assert_eq!(read(r#"{"spreads": [12, 40]}"#), Ok(vec![12, 40]));
    assert_eq!(read(r#"{"spreads": []}"#), Ok(vec![]));
    // Anything else is refused rather than guessed at.
    assert!(read(r#"{"pages": [1]}"#).is_err());
    assert!(read(r#"{"spreads": [1, -2]}"#).is_err());
    assert!(read(r#"{"spreads": ["3"]}"#).is_err());
    assert!(read("not json").is_err());
    assert!(read_spread_labels(&dir.path().join("missing.json")).is_err());
}

#[test]
fn without_a_covers_folder_there_is_no_cover_by_convention() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Vol 1.cbz"), b"x").unwrap();
    assert_eq!(cover_by_convention(&dir.path().join("Vol 1.cbz")), None);
}

#[test]
fn covers_go_to_books_by_position_in_natural_order() {
    let dir = series(
        &["Vol 1.cbz", "Vol 2.cbz", "Vol 10.cbz", "notes.txt"],
        &["c.png", "a.jpg", "b.jpg", "readme.txt"],
    );
    assert_eq!(cover_for(&dir, "Vol 1.cbz").as_deref(), Some("a.jpg"));
    assert_eq!(cover_for(&dir, "Vol 2.cbz").as_deref(), Some("b.jpg"));
    assert_eq!(cover_for(&dir, "Vol 10.cbz").as_deref(), Some("c.png"));
}

#[test]
fn a_book_past_the_last_cover_has_none() {
    let dir = series(&["Vol 1.cbz", "Vol 2.cbz"], &["a.jpg"]);
    assert_eq!(cover_for(&dir, "Vol 2.cbz"), None);
}

#[test]
fn earlier_output_beside_the_books_does_not_shift_the_covers() {
    let dir = series(
        &[
            "Vol 1.cbz",
            "Vol 1 (mangapress).cbz",
            "Vol 1_kcc0.cbz",
            "Vol 1.epub",
            "Vol 2.cbz",
        ],
        &["a.jpg", "b.jpg"],
    );
    assert_eq!(cover_for(&dir, "Vol 2.cbz").as_deref(), Some("b.jpg"));
}

#[test]
fn folders_are_counted_among_folders_and_covers_is_not_one_of_them() {
    // "Covers" sorts before both; upstream would count it as a book.
    let dir = series(&["Vol 1/", "Vol 2/", "Vol 3.cbz"], &["a.jpg", "b.jpg"]);
    assert_eq!(cover_for(&dir, "Vol 1").as_deref(), Some("a.jpg"));
    assert_eq!(cover_for(&dir, "Vol 2").as_deref(), Some("b.jpg"));
}

#[test]
fn a_cover_named_like_the_book_is_its_cover_wherever_it_sorts() {
    let dir = series(
        &["Vol 1.cbz", "Vol 2.cbz", "Vol 3.cbz"],
        &["VOL 3.jpg", "Vol 2.png"],
    );
    assert_eq!(cover_for(&dir, "Vol 3.cbz").as_deref(), Some("VOL 3.jpg"));
    assert_eq!(cover_for(&dir, "Vol 2.cbz").as_deref(), Some("Vol 2.png"));
    // Covers here are named after books, so the one without its own
    // gets none — not "Vol 2.png", which position would hand it.
    assert_eq!(cover_for(&dir, "Vol 1.cbz"), None);

    // A folder's whole name is its title, dots included.
    let dir = series(&["Vol. 1/", "Vol. 2/"], &["Vol. 2.jpg"]);
    assert_eq!(cover_for(&dir, "Vol. 2").as_deref(), Some("Vol. 2.jpg"));
    assert_eq!(cover_for(&dir, "Vol. 1"), None);
}

fn solid_gray_png(size: u32, gray: u8) -> Vec<u8> {
    let img = image::GrayImage::from_pixel(size, size, image::Luma([gray]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageLuma8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

fn minimal_pipeline_options() -> PipelineOptions {
    PipelineOptions {
        profile: Profile::by_code("KV").unwrap(),
        width_override: None,
        height_override: None,
        manga_style: false,
        cropping: CroppingMode::Disabled,
        cropping_power: 1.0,
        cropping_minimum: 0.0,
        preserve_margin_percent: 0.0,
        inter_panel_crop: mangapress_core::crop::inter_panel::InterPanelMode::Disabled,
        splitter: SplitterMode::Split,
        upscale: true,
        stretch: false,
        wallpaper: false,
        white_borders: false,
        black_borders: false,
        no_rotate: false,
        rotate_first: false,
        maximize_strips: false,
        color_autocontrast: false,
        webtoon: false,
        force_color: false,
        force_png_rgb: false,
        png_legacy: false,
        no_quantize: false,
        no_processing: false,
        rotate_right: false,
        force_png: false,
        output_format: OutputFormat::Epub,
        gamma: None,
        autolevel: false,
        noautocontrast: true,
        erase_rainbow: false,
        jpeg_quality: None,
    }
}

#[test]
fn process_chapter_pages_preserves_input_order_despite_parallel_processing() {
    // Distinct, monotonically increasing solid gray levels: every stage
    // a flat-color page passes through here (gamma, resize, JPEG
    // encoding) is order-preserving on brightness, so if the *output*
    // pages come back in strictly increasing order too, the parallel
    // fan-out inside process_chapter_pages didn't reorder, drop, or
    // duplicate any page relative to its position in the input slice --
    // the one correctness property that actually matters about running
    // this in parallel rather than one page at a time.
    let gray_levels = [10u8, 60, 110, 160, 210];
    let pages: Vec<Page> = gray_levels
        .iter()
        .enumerate()
        .map(|(index, &g)| Page {
            source_path: Some(PathBuf::from(format!("page-{index}.png"))),
            extension: "png".to_string(),
            bytes: solid_gray_png(40, g),
            ..Default::default()
        })
        .collect();

    let options = minimal_pipeline_options();
    let processed = process_chapter_pages(&pages, &options, false, |_, _| Ok(()))
        .unwrap()
        .pages;
    assert_eq!(processed.len(), gray_levels.len());
    for (source, result) in pages.iter().zip(&processed) {
        assert_eq!(result.source_path, source.source_path);
    }

    let output_grays: Vec<u8> = processed
        .iter()
        .map(|page| {
            let img = image::load_from_memory(&page.bytes).unwrap().to_luma8();
            img.get_pixel(img.width() / 2, img.height() / 2)[0]
        })
        .collect();

    for pair in output_grays.windows(2) {
        assert!(
            pair[0] < pair[1],
            "expected strictly increasing gray levels in input order, got {output_grays:?}"
        );
    }
}

#[test]
fn process_chapter_pages_reports_each_source_page_in_stable_order() {
    let pages: Vec<Page> = (0..6)
        .map(|_| Page {
            extension: "png".to_string(),
            bytes: solid_gray_png(20, 128),
            ..Default::default()
        })
        .collect();
    let options = minimal_pipeline_options();

    let calls = std::sync::Mutex::new(Vec::new());
    process_chapter_pages(&pages, &options, false, |done, page| {
        calls.lock().unwrap().push((done, page));
        Ok(())
    })
    .unwrap();

    assert_eq!(
        calls.into_inner().unwrap(),
        vec![(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]
    );
}
