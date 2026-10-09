use super::*;

#[test]
fn scribe_and_colorsoft_profiles_default_to_90() {
    assert_eq!(
        default_jpeg_quality(crate::profile::Profile::by_code("KS").unwrap()),
        90
    );
    assert_eq!(
        default_jpeg_quality(crate::profile::Profile::by_code("KCS").unwrap()),
        90
    );
    assert_eq!(
        default_jpeg_quality(crate::profile::Profile::by_code("KS3").unwrap()),
        90
    );
}

#[test]
fn other_profiles_default_to_85() {
    assert_eq!(
        default_jpeg_quality(crate::profile::Profile::by_code("KV").unwrap()),
        85
    );
}

#[test]
fn a_custom_size_gives_sixteen_gray_levels_whatever_the_device_had() {
    use crate::profile::{Palette, Profile};
    let k1 = Profile::by_code("K1").unwrap();
    let k2 = Profile::by_code("K2").unwrap();
    assert_eq!(effective_palette(k1, None, None), Palette::Gray4);
    assert_eq!(effective_palette(k2, None, None), Palette::Gray15);
    for (width, height) in [
        (Some(800), None),
        (None, Some(1200)),
        (Some(800), Some(1200)),
    ] {
        assert_eq!(effective_palette(k1, width, height), Palette::Gray16);
        assert_eq!(effective_palette(k2, width, height), Palette::Gray16);
    }
    // Zero is "not overridden", as it is for the resolution.
    assert_eq!(effective_palette(k1, Some(0), Some(0)), Palette::Gray4);
}

#[test]
fn a_custom_size_makes_scribe_and_colorsoft_ordinary_devices_for_the_jpeg_quality() {
    use crate::profile::Profile;
    for code in ["KS", "KS3", "KCS", "KSCS"] {
        let profile = Profile::by_code(code).unwrap();
        assert_eq!(
            effective_default_jpeg_quality(profile, None, None),
            90,
            "{code}"
        );
        assert_eq!(
            effective_default_jpeg_quality(profile, Some(1000), None),
            85,
            "{code}"
        );
        assert_eq!(
            effective_default_jpeg_quality(profile, None, Some(1000)),
            85,
            "{code}"
        );
        assert_eq!(
            effective_default_jpeg_quality(profile, Some(0), Some(0)),
            90,
            "{code}"
        );
    }
    let kv = Profile::by_code("KV").unwrap();
    assert_eq!(
        effective_default_jpeg_quality(kv, Some(1000), Some(1000)),
        85
    );
}

#[test]
fn the_quality_asked_for_wins_over_every_default() {
    let mut options = options();
    options.profile = crate::profile::Profile::by_code("KS3").unwrap();
    options.jpeg_quality = Some(60);
    assert_eq!(options.jpeg_quality(), 60);
    options.width_override = Some(1000);
    assert_eq!(options.jpeg_quality(), 60);
    options.jpeg_quality = None;
    assert_eq!(options.jpeg_quality(), 85);
    options.width_override = None;
    assert_eq!(options.jpeg_quality(), 90);
}

fn options() -> PipelineOptions {
    PipelineOptions {
        profile: crate::profile::Profile::by_code("K11").unwrap(),
        width_override: None,
        height_override: None,
        manga_style: false,
        cropping: CroppingMode::Margins,
        cropping_power: 1.0,
        cropping_minimum: 0.0,
        preserve_margin_percent: 0.0,
        inter_panel_crop: crate::crop::inter_panel::InterPanelMode::Disabled,
        splitter: SplitterMode::Split,
        // Off, with pages smaller than the device, so nothing is resized
        // and output sizes are exactly the cropped sizes.
        upscale: false,
        stretch: false,
        wallpaper: false,
        white_borders: false,
        black_borders: false,
        rotate_right: false,
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
        output_format: OutputFormat::Epub,
        force_png: false,
        gamma: None,
        autolevel: false,
        noautocontrast: false,
        erase_rainbow: false,
        jpeg_quality: Some(100),
    }
}

#[test]
fn kindle_dx_cbz_target_does_not_change_the_device_table_or_other_formats() {
    let mut options = options();
    options.profile = crate::profile::Profile::by_code("KDX").unwrap();
    assert_eq!(
        options.profile.effective_resolution(None, None),
        (824, 1000)
    );
    for (format, expected) in [
        (OutputFormat::Cbz, (824, 1200)),
        (OutputFormat::Epub, (824, 1000)),
        (OutputFormat::Pdf, (824, 1000)),
    ] {
        options.output_format = format;
        assert_eq!(options.target_resolution(), expected);
    }
    for profile in crate::profile::PROFILES.iter().filter(|p| p.code != "KDX") {
        for format in [OutputFormat::Cbz, OutputFormat::Epub, OutputFormat::Pdf] {
            let expected = match (format, profile.code) {
                (OutputFormat::Epub, "KS3" | "KSCS") => (1920, 2648),
                _ => (profile.width, profile.height),
            };
            assert_eq!(format.target_resolution(profile, None, None), expected);
        }
    }
}

#[test]
fn either_custom_dimension_disables_the_kindle_dx_cbz_target() {
    let mut options = options();
    options.profile = crate::profile::Profile::by_code("KDX").unwrap();
    options.output_format = OutputFormat::Cbz;
    for (width, height, expected) in [
        (Some(824), None, (824, 1000)),
        (None, Some(1000), (824, 1000)),
        (Some(900), None, (900, 1000)),
        (None, Some(1400), (824, 1400)),
        (Some(900), Some(1400), (900, 1400)),
    ] {
        options.width_override = width;
        options.height_override = height;
        assert_eq!(options.target_resolution(), expected);
    }
}

#[test]
fn scribe_epub_width_cap_does_not_apply_to_other_formats_or_custom_dimensions() {
    let mut options = options();
    for code in ["KS3", "KSCS"] {
        options.profile = crate::profile::Profile::by_code(code).unwrap();
        assert_eq!(
            (options.profile.width, options.profile.height),
            (1986, 2648)
        );
        for (format, expected) in [
            (OutputFormat::Epub, (1920, 2648)),
            (OutputFormat::Cbz, (1986, 2648)),
            (OutputFormat::Pdf, (1986, 2648)),
        ] {
            options.output_format = format;
            options.width_override = None;
            options.height_override = None;
            assert_eq!(options.target_resolution(), expected);
            options.width_override = Some(1986);
            assert_eq!(options.target_resolution(), (1986, 2648));
            options.width_override = None;
            options.height_override = Some(2648);
            assert_eq!(options.target_resolution(), (1986, 2648));
        }
    }
}

fn png(page: image::RgbImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(page)
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .unwrap();
    bytes
}

fn decode(page: &ProcessedPage) -> image::GrayImage {
    image::load_from_memory(&page.bytes).unwrap().to_luma8()
}

/// A white page with a block of content inset by `margin` on every side:
/// dark gray (40), a lighter band (200) through it, and — if `colored` —
/// a saturated patch, which is what makes upstream call a page color.
fn page_with_margins((width, height): (u32, u32), margin: u32, colored: bool) -> image::RgbImage {
    image::RgbImage::from_fn(width, height, |x, y| {
        let inside = x >= margin && x < width - margin && y >= margin && y < height - margin;
        if !inside {
            image::Rgb([255, 255, 255])
        } else if colored && x < width / 2 && y < height / 3 {
            image::Rgb([200, 60, 60])
        } else if y % 40 < 10 {
            image::Rgb([200, 200, 200])
        } else {
            image::Rgb([40, 40, 40])
        }
    })
}

fn extrema(page: &image::GrayImage) -> (u8, u8) {
    page.pixels()
        .fold((255, 0), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])))
}

#[test]
fn a_color_page_is_not_autocontrasted_but_a_gray_one_is() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    // No margins: the page's values run 40..200, wide enough (160) that
    // autocontrast would stretch them to 0..255.
    let gray = process_page(
        &png(page_with_margins((300, 450), 0, false)),
        &options,
        false,
    )
    .unwrap();
    let (low, high) = extrema(&decode(&gray[0]));
    assert!(low < 10 && high > 245, "gray page stretched: {low}..{high}");

    let color = process_page(
        &png(page_with_margins((300, 450), 0, true)),
        &options,
        false,
    )
    .unwrap();
    let (low, high) = extrema(&decode(&color[0]));
    assert!(
        low > 25 && high < 215,
        "color page left at its own contrast: {low}..{high}"
    );
}

#[test]
fn a_color_first_page_is_left_uncropped() {
    let cover = png(page_with_margins((400, 600), 30, true));
    let first = process_page(&cover, &options(), true).unwrap();
    assert_eq!(decode(&first[0]).dimensions(), (400, 600));

    // The same page anywhere else in the book loses its margins...
    let later = process_page(&cover, &options(), false).unwrap();
    let (width, height) = decode(&later[0]).dimensions();
    assert!(width < 350 && height < 550, "{width}x{height}");

    // ...and so does a first page that isn't color.
    let gray_first = process_page(
        &png(page_with_margins((400, 600), 30, false)),
        &options(),
        true,
    )
    .unwrap();
    assert!(decode(&gray_first[0]).width() < 350);
}

#[test]
fn margins_are_cropped_before_a_spread_is_split() {
    // Two 900px-wide pages side by side with 150px of margin on the left
    // and 50px on the right. Cropping first leaves 1800px, split down the
    // gutter into two equal halves; splitting first would cut 50px into
    // the left page and give halves of different widths.
    let spread = image::RgbImage::from_fn(2000, 1400, |x, y| {
        let inside = (150..1950).contains(&x) && (25..1375).contains(&y);
        if inside {
            image::Rgb([60, 60, 60])
        } else {
            image::Rgb([255, 255, 255])
        }
    });
    let halves = process_page(&png(spread), &options(), false).unwrap();
    assert_eq!(halves.len(), 2);
    let (first, second) = (decode(&halves[0]), decode(&halves[1]));
    assert_eq!(first.width(), second.width());
    assert!((899..=901).contains(&first.width()), "{}", first.width());
    assert_eq!(
        (halves[0].role, halves[1].role),
        (spread::PageRole::SplitFirst, spread::PageRole::SplitSecond)
    );
}

#[test]
fn a_dark_page_is_flagged_for_a_black_page_background_unless_borders_are_forced_white() {
    let dark = image::RgbImage::from_fn(300, 450, |x, y| {
        if (100..200).contains(&x) && (150..300).contains(&y) {
            image::Rgb([230, 230, 230])
        } else {
            image::Rgb([10, 10, 10])
        }
    });
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    let page = process_page(&png(dark.clone()), &options, false).unwrap();
    assert!(page[0].black_background);

    options.white_borders = true;
    let page = process_page(&png(dark), &options, false).unwrap();
    assert!(!page[0].black_background);

    let light = process_page(
        &png(page_with_margins((300, 450), 20, false)),
        &options,
        false,
    )
    .unwrap();
    assert!(!light[0].black_background);
}

#[test]
fn a_quantized_page_is_a_palette_png_unless_upstream_makes_it_grayscale() {
    let container = |profile: &str, format: OutputFormat, custom_width: Option<u32>| {
        let mut options = options();
        options.profile = crate::profile::Profile::by_code(profile).unwrap();
        options.output_format = format;
        options.width_override = custom_width;
        quantized_container(&options)
    };
    // EPUB and CBZ are palette PNG on every device — a Kindle included,
    // where upstream would write a GIF.
    for profile in ["K11", "K2", "KoC", "Rmk2"] {
        assert_eq!(
            container(profile, OutputFormat::Epub, None),
            Container::IndexedPng,
            "{profile}"
        );
    }
    assert_eq!(
        container("K11", OutputFormat::Cbz, None),
        Container::IndexedPng
    );
    // PDF, and CBZ for the four oldest Kindles, go back to grayscale...
    assert_eq!(
        container("K11", OutputFormat::Pdf, None),
        Container::GrayPng
    );
    for profile in ["K1", "K2", "K34", "KDX"] {
        assert_eq!(
            container(profile, OutputFormat::Cbz, None),
            Container::GrayPng
        );
    }
    // ...unless the resolution is custom, which upstream no longer
    // treats as one of those four.
    assert_eq!(
        container("K2", OutputFormat::Cbz, Some(1000)),
        Container::IndexedPng
    );
}

#[test]
fn force_png_writes_png_with_the_same_pixels_in_either_container() {
    let page = png(page_with_margins((300, 450), 20, false));
    let mut options = options();
    options.force_png = true;

    // A Kindle profile's EPUB: palette PNG, 4 bits per pixel.
    let indexed = process_page(&page, &options, false).unwrap();
    assert_eq!(indexed[0].extension, "png");
    assert_eq!(&indexed[0].bytes[1..4], b"PNG");
    assert_eq!(indexed[0].bytes[24], 4, "bit depth");
    assert_eq!(indexed[0].bytes[25], 3, "color type: palette");

    options.output_format = OutputFormat::Pdf;
    let gray = process_page(&page, &options, false).unwrap();
    assert_eq!(gray[0].bytes[24], 8, "bit depth");
    assert_eq!(decode(&gray[0]), decode(&indexed[0]));
    let levels = options.profile.palette.level_values();
    assert!(decode(&gray[0]).pixels().all(|p| levels.contains(&p[0])));
}

/// A 2000x1400 spread with content edge to edge: its left half dark,
/// its right half light, so the halves can be told apart.
fn two_tone_spread() -> image::RgbImage {
    image::RgbImage::from_fn(2000, 1400, |x, _| {
        if x < 1000 {
            image::Rgb([40, 40, 40])
        } else {
            image::Rgb([200, 200, 200])
        }
    })
}

fn mean(page: &image::GrayImage) -> f64 {
    page.pixels().map(|p| p[0] as f64).sum::<f64>() / (page.width() * page.height()) as f64
}

#[test]
fn rotate_first_puts_the_whole_spread_before_its_halves() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.splitter = SplitterMode::Both;
    let roles = |options: &PipelineOptions| -> Vec<spread::PageRole> {
        process_page(&png(two_tone_spread()), options, false)
            .unwrap()
            .iter()
            .map(|page| page.role)
            .collect()
    };
    use spread::PageRole::{Rotated, SplitFirst, SplitSecond};
    assert_eq!(roles(&options), [SplitFirst, SplitSecond, Rotated]);
    options.rotate_first = true;
    assert_eq!(roles(&options), [Rotated, SplitFirst, SplitSecond]);
}

#[test]
fn no_rotate_keeps_the_whole_spread_upright_and_only_shrinks_it_when_oversized() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.splitter = SplitterMode::Rotate;
    options.upscale = true;
    options.no_rotate = true;
    // Kobo: 1072x1448. A 2000x1400 spread fits two widths by one height,
    // so it is left exactly as it is, landscape.
    options.profile = crate::profile::Profile::by_code("KoC").unwrap();
    let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
    assert_eq!(page[0].role, spread::PageRole::Rotated);
    assert_eq!(decode(&page[0]).dimensions(), (2000, 1400));

    // The same on a Kindle profile, where upstream would cap it at 1920px.
    options.profile = crate::profile::Profile::by_code("K11").unwrap();
    let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
    assert_eq!(decode(&page[0]).dimensions(), (2000, 1400));

    // Larger than two screens wide, it is shrunk to exactly that.
    let huge = image::RgbImage::from_pixel(4288, 1448, image::Rgb([128, 128, 128]));
    let page = process_page(&png(huge), &options, false).unwrap();
    assert_eq!(decode(&page[0]).dimensions(), (2144, 724));

    // Rotated as usual without the flag.
    options.no_rotate = false;
    let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
    let (width, height) = decode(&page[0]).dimensions();
    assert!(height > width);
}

#[test]
fn maximize_strips_stacks_the_first_read_half_on_top() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.noautocontrast = true;
    options.maximize_strips = true;
    let page = process_page(&png(two_tone_spread()), &options, false).unwrap();
    assert_eq!(page.len(), 1, "no spread handling at all");
    assert_eq!(page[0].role, spread::PageRole::Normal);
    let stacked = decode(&page[0]);
    // 1000x2800, fitted into 1072x1448.
    assert_eq!(stacked.dimensions(), (517, 1448));
    let top = image::imageops::crop_imm(&stacked, 0, 0, 517, 700).to_image();
    let bottom = image::imageops::crop_imm(&stacked, 0, 748, 517, 700).to_image();
    // Left-to-right: the dark left half on top.
    assert!(mean(&top) < 60.0 && mean(&bottom) > 180.0);

    options.manga_style = true;
    let stacked = decode(&process_page(&png(two_tone_spread()), &options, false).unwrap()[0]);
    let top = image::imageops::crop_imm(&stacked, 0, 0, 517, 700).to_image();
    assert!(mean(&top) > 180.0, "right-to-left: the right half on top");
}

#[test]
fn black_borders_flag_every_page_and_win_over_white_borders() {
    let light = png(page_with_margins((300, 450), 20, false));
    let mut options = options();
    options.black_borders = true;
    assert!(process_page(&light, &options, false).unwrap()[0].black_background);
    options.white_borders = true;
    assert!(process_page(&light, &options, false).unwrap()[0].black_background);
}

#[test]
fn color_autocontrast_stretches_a_color_page_too() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.color_autocontrast = true;
    let color = process_page(
        &png(page_with_margins((300, 450), 0, true)),
        &options,
        false,
    )
    .unwrap();
    let (low, high) = extrema(&decode(&color[0]));
    assert!(low < 10 && high > 245, "{low}..{high}");
}

#[test]
fn png_legacy_and_no_quantize_store_plain_grayscale() {
    let container = |format: OutputFormat, legacy: bool, no_quantize: bool| {
        let mut options = options();
        options.output_format = format;
        options.png_legacy = legacy;
        options.no_quantize = no_quantize;
        quantized_container(&options)
    };
    assert_eq!(
        container(OutputFormat::Epub, true, false),
        Container::GrayPng
    );
    assert_eq!(
        container(OutputFormat::Epub, false, true),
        Container::GrayPng
    );
    assert_eq!(
        container(OutputFormat::Cbz, true, false),
        Container::GrayPng
    );
    assert_eq!(
        container(OutputFormat::Cbz, false, true),
        Container::GrayPng
    );

    let page = png(page_with_margins((300, 450), 20, false));
    let mut options = options();
    options.force_png = true;
    options.png_legacy = true;
    let legacy = process_page(&page, &options, false).unwrap();
    assert_eq!(legacy[0].extension, "png");
    assert_eq!(legacy[0].bytes[24], 8, "bit depth");
    let levels = options.profile.palette.level_values();
    assert!(decode(&legacy[0]).pixels().all(|p| levels.contains(&p[0])));

    options.png_legacy = false;
    options.no_quantize = true;
    let unquantized = decode(&process_page(&page, &options, false).unwrap()[0]);
    assert!(
        unquantized.pixels().any(|p| !levels.contains(&p[0])),
        "all 256 levels kept"
    );
}

#[test]
fn no_processing_hands_the_source_back_untouched() {
    let source = png(page_with_margins((300, 450), 20, true));
    let mut options = options();
    options.no_processing = true;
    let page = process_page(&source, &options, true).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].extension, "png");
    assert_eq!(page[0].bytes, source);
}

#[test]
fn bmp_passthrough_keeps_its_codec_except_in_epub() {
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        32,
        48,
        image::Rgb([40, 80, 160]),
    ))
    .write_to(&mut encoded, ImageFormat::Bmp)
    .unwrap();
    let source = encoded.into_inner();
    let mut options = options();
    options.no_processing = true;
    for format in [OutputFormat::Cbz, OutputFormat::Pdf] {
        options.output_format = format;
        let pages = process_page(&source, &options, true).unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].extension, "bmp");
        assert_eq!(pages[0].bytes, source);
    }
    options.output_format = OutputFormat::Epub;
    let error = process_page(&source, &options, true).unwrap_err();
    assert!(matches!(error, crate::Error::Encode(_)));
    assert!(error
        .to_string()
        .contains("Remove --noprocessing or choose CBZ"));
    options.no_processing = false;
    assert!(process_page(&source, &options, true).is_ok());
}

#[test]
fn oversized_images_are_refused_in_normal_and_passthrough_modes() {
    let source = crate::test_support::oversized_bmp();
    for no_processing in [false, true] {
        let mut options = options();
        options.no_processing = no_processing;
        assert!(matches!(
            process_page(&source, &options, true),
            Err(crate::Error::ImageTooLarge { .. })
        ));
    }
}

#[test]
fn force_color_keeps_a_color_page_in_color_and_a_gray_page_gray() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.force_color = true;

    let color = process_page(
        &png(page_with_margins((300, 450), 0, true)),
        &options,
        false,
    )
    .unwrap();
    let decoded = image::load_from_memory(&color[0].bytes).unwrap();
    assert!(decoded.color().has_color());
    // The saturated patch is still red: far more red than green.
    let patch = decoded.to_rgb8().get_pixel(40, 40).0;
    assert!(patch[0] > 170 && patch[1] < 90, "{patch:?}");

    let gray = process_page(
        &png(page_with_margins((300, 450), 0, false)),
        &options,
        false,
    )
    .unwrap();
    assert!(!image::load_from_memory(&gray[0].bytes)
        .unwrap()
        .color()
        .has_color());
}

#[test]
fn force_png_rgb_saves_a_color_page_as_png() {
    let mut options = options();
    options.cropping = CroppingMode::Disabled;
    options.force_color = true;
    options.force_png = true;
    options.output_format = OutputFormat::Cbz;
    let page = png(page_with_margins((300, 450), 0, true));
    // Without it, a color page stays JPEG even under --forcepng.
    assert_eq!(
        process_page(&page, &options, false).unwrap()[0].extension,
        "jpg"
    );
    options.force_png_rgb = true;
    let saved = process_page(&page, &options, false).unwrap();
    assert_eq!(saved[0].extension, "png");
    assert!(image::load_from_memory(&saved[0].bytes)
        .unwrap()
        .color()
        .has_color());
}

#[test]
fn webtoon_pages_are_never_margin_cropped_split_or_autocontrasted() {
    let mut options = options();
    options.webtoon = true;
    options.splitter = SplitterMode::Both;

    // Wide enough to be a spread, and with contrast autocontrast would
    // stretch (40..200): it comes out as one page, shrunk to the
    // device's width, at its own contrast.
    let out = process_page(&png(two_tone_spread()), &options, false).unwrap();
    assert_eq!(out.len(), 1);
    let page = decode(&out[0]);
    assert_eq!(page.dimensions(), (1072, 750));
    let (low, high) = extrema(&page);
    assert!(low > 15 && high < 225, "{low}..{high}");

    // Margins stay.
    let with_margins = png(page_with_margins((400, 600), 30, false));
    let out = process_page(&with_margins, &options, false).unwrap();
    assert_eq!(decode(&out[0]).dimensions(), (400, 600));
}
