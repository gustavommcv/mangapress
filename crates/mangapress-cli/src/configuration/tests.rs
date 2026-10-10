use super::*;
use clap::Parser;
use std::ffi::OsString;
use std::path::Path;

fn arguments(input: &Path, options: &[&str]) -> Cli {
    Cli::parse_from(
        [OsString::from("mangapress"), input.as_os_str().to_owned()]
            .into_iter()
            .chain(options.iter().map(OsString::from)),
    )
}

fn initial_failure() -> RunFailure {
    RunFailure::new(
        "conversion_failed",
        "conversion",
        false,
        "conversion failed",
        "conversion failed",
    )
}

fn settings(input: &Path, options: &[&str]) -> ResolvedConversion {
    resolve(arguments(input, options), &mut initial_failure()).unwrap()
}

fn processing_switches(options: &PipelineOptions) -> [(&'static str, bool); 20] {
    [
        ("--manga-style", options.manga_style),
        ("--upscale", options.upscale),
        ("--stretch", options.stretch),
        ("--wallpaper", options.wallpaper),
        ("--whiteborders", options.white_borders),
        ("--blackborders", options.black_borders),
        ("--norotate", options.no_rotate),
        ("--rotatefirst", options.rotate_first),
        ("--maximizestrips", options.maximize_strips),
        ("--colorautocontrast", options.color_autocontrast),
        ("--forcecolor", options.force_color),
        ("--force-png-rgb", options.force_png_rgb),
        ("--pnglegacy", options.png_legacy),
        ("--noquantize", options.no_quantize),
        ("--noprocessing", options.no_processing),
        ("--rotateright", options.rotate_right),
        ("--forcepng", options.force_png),
        ("--autolevel", options.autolevel),
        ("--noautocontrast", options.noautocontrast),
        ("--eraserainbow", options.erase_rainbow),
    ]
}

#[test]
fn automatic_format_is_resolved_before_the_processing_target() {
    let input = tempfile::tempdir().unwrap();
    for (code, format, processing_format, target) in [
        ("KDX", Format::Cbz, OutputFormat::Cbz, (824, 1200)),
        ("K11", Format::Cbz, OutputFormat::Cbz, (1072, 1448)),
        ("KoC", Format::Epub, OutputFormat::Epub, (1072, 1448)),
        ("KS3", Format::Cbz, OutputFormat::Cbz, (1986, 2648)),
        ("Rmk2", Format::Pdf, OutputFormat::Pdf, (1404, 1872)),
    ] {
        let resolved = settings(input.path(), &["--profile", code]);
        assert_eq!(resolved.cli.format, format, "{code}");
        assert_eq!(resolved.output_format, processing_format, "{code}");
        assert_eq!((resolved.width, resolved.height), target, "{code}");
        let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
        assert_eq!(options.target_resolution(), target, "{code}");
    }
}

#[test]
fn explicit_format_overrides_the_device_family_default() {
    let input = tempfile::tempdir().unwrap();
    for (code, name, expected) in [
        ("Rmk2", "epub", Format::Epub),
        ("Rmk2", "cbz", Format::Cbz),
        ("KDX", "pdf", Format::Pdf),
    ] {
        let resolved = settings(input.path(), &["--profile", code, "--format", name]);
        assert_eq!(resolved.cli.format, expected);
        assert_eq!(resolved.output_format, pipeline_format(expected));
    }
}

#[test]
fn custom_resolution_and_other_cli_inputs_survive_resolution() {
    let input = tempfile::tempdir().unwrap();
    for (options, expected) in [
        (
            vec![
                "--profile",
                "OTHER",
                "--customwidth",
                "120",
                "--customheight",
                "180",
            ],
            (120, 180),
        ),
        (
            vec!["--profile", "KDX", "--customwidth", "700"],
            (700, 1000),
        ),
        (
            vec!["--profile", "KS3", "--customheight", "2648"],
            (1986, 2648),
        ),
    ] {
        let mut options = options;
        options.extend([
            "--title",
            "Chosen title",
            "--output",
            "chosen.epub",
            "--dry-run",
            "--json-events",
        ]);
        let resolved = settings(input.path(), &options);
        assert_eq!((resolved.width, resolved.height), expected);
        assert_eq!(resolved.input, input.path());
        assert_eq!(resolved.input_path, absolute_display(input.path()));
        assert_eq!(resolved.cli.input.as_deref(), Some(input.path()));
        assert_eq!(resolved.cli.title.as_deref(), Some("Chosen title"));
        assert_eq!(
            resolved.cli.output.as_deref(),
            Some(Path::new("chosen.epub"))
        );
        assert!(resolved.cli.dry_run && resolved.cli.json_events);
    }
}

#[test]
fn unknown_profile_precedes_missing_input_and_keeps_the_suggestion() {
    let directory = tempfile::tempdir().unwrap();
    let mut failure = initial_failure();
    let error = resolve(
        arguments(&directory.path().join("missing"), &["--profile", "k11"]),
        &mut failure,
    )
    .unwrap_err();
    assert_eq!(failure.code, "unknown_profile");
    assert_eq!(failure.stage, "configuration");
    assert!(failure.recoverable);
    assert_eq!(failure.message, "Unknown device profile 'k11'.");
    assert_eq!(
        failure.diagnostic,
        "unknown device profile 'k11' -- did you mean 'K11'? (see --list-profiles)"
    );
    assert_eq!(error.to_string(), failure.diagnostic);
    assert!(failure.path.is_none());
}

#[test]
fn missing_input_precedes_resolution_and_nested_toc_validation() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("missing");
    let mut failure = initial_failure();
    let error = resolve(
        arguments(
            &input,
            &["--profile", "OTHER", "--format", "cbz", "--nested-toc"],
        ),
        &mut failure,
    )
    .unwrap_err();
    assert_eq!(failure.code, "input_not_found");
    assert_eq!(failure.stage, "inspect");
    assert!(failure.recoverable);
    assert_eq!(
        failure.path.as_deref(),
        Some(absolute_display(&input).as_str())
    );
    assert_eq!(failure.message, "The input path does not exist.");
    assert_eq!(
        error.to_string(),
        format!("input path does not exist: {}", input.display())
    );
}

#[test]
fn invalid_resolution_precedes_nested_toc_validation() {
    let input = tempfile::tempdir().unwrap();
    for options in [
        vec!["--customwidth", "0"],
        vec!["--profile", "OTHER", "--customwidth", "120"],
        vec!["--profile", "OTHER", "--customheight", "180"],
    ] {
        let mut options = options;
        options.extend(["--format", "cbz", "--nested-toc"]);
        let mut failure = initial_failure();
        let error = resolve(arguments(input.path(), &options), &mut failure).unwrap_err();
        assert_eq!(failure.code, "invalid_resolution");
        assert_eq!(failure.stage, "configuration");
        assert!(failure.recoverable);
        assert_eq!(
            failure.message,
            "Set both a target width and height for this device profile."
        );
        assert_eq!(error.to_string(), failure.diagnostic);
        assert!(failure.path.is_none());
    }
}

#[test]
fn nested_toc_compatibility_uses_the_resolved_format() {
    let input = tempfile::tempdir().unwrap();
    for (code, flag, name) in [
        ("KDX", "cbz", "Cbz"),
        ("K11", "cbz", "Cbz"),
        ("Rmk2", "pdf", "Pdf"),
    ] {
        let mut failure = initial_failure();
        let error = resolve(
            arguments(
                input.path(),
                &["--profile", code, "--format", flag, "--nested-toc"],
            ),
            &mut failure,
        )
        .unwrap_err();
        assert_eq!(failure.code, "nested_toc_unsupported_format");
        assert_eq!(failure.stage, "configuration");
        assert_eq!(
            error.to_string(),
            format!("--nested-toc requires --format epub, got --format {name}")
        );
        assert!(failure
            .diagnostic
            .contains("0012-nested-toc-for-combined-volumes.md"));
    }
    for options in [
        vec!["--profile", "KDX", "--format", "epub", "--nested-toc"],
        vec!["--profile", "K11", "--format", "epub", "--nested-toc"],
    ] {
        let resolved = settings(input.path(), &options);
        assert_eq!(resolved.cli.format, Format::Epub);
        assert!(resolved.cli.nested_toc);
    }
}

#[test]
fn nested_toc_makes_an_automatic_format_an_epub_on_every_device() {
    let input = tempfile::tempdir().unwrap();
    for code in ["KDX", "K11", "KoC", "KS3", "Rmk2"] {
        let resolved = settings(input.path(), &["--profile", code, "--nested-toc"]);
        assert_eq!(resolved.cli.format, Format::Epub, "{code}");
        assert_eq!(resolved.output_format, OutputFormat::Epub, "{code}");
        assert!(resolved.cli.nested_toc, "{code}");
    }
}

#[test]
fn processing_options_forward_explicit_cli_values() {
    let input = tempfile::tempdir().unwrap();
    let resolved = settings(
        input.path(),
        &[
            "--profile",
            "K11",
            "--format",
            "pdf",
            "--customwidth",
            "120",
            "--customheight",
            "180",
            "--manga-style",
            "--upscale",
            "--stretch",
            "--wallpaper",
            "--whiteborders",
            "--blackborders",
            "--norotate",
            "--rotatefirst",
            "--maximizestrips",
            "--colorautocontrast",
            "--forcecolor",
            "--force-png-rgb",
            "--pnglegacy",
            "--noquantize",
            "--noprocessing",
            "--rotateright",
            "--forcepng",
            "--autolevel",
            "--noautocontrast",
            "--eraserainbow",
            "--croppingpower",
            "2",
            "--croppingminimum",
            "60",
            "--preservemargin",
            "5",
            "--gamma",
            "0.5",
            "--jpeg-quality",
            "53",
        ],
    );
    let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
    assert_eq!(options.profile.code, "K11");
    assert_eq!(options.width_override, Some(120));
    assert_eq!(options.height_override, Some(180));
    assert_eq!(options.output_format, OutputFormat::Pdf);
    assert_eq!(options.cropping_power, 2.0);
    assert_eq!(options.cropping_minimum, 60.0);
    assert_eq!(options.preserve_margin_percent, 5.0);
    assert_eq!(options.gamma, Some(0.5));
    assert_eq!(options.jpeg_quality, Some(53));
    for (name, value) in processing_switches(&options) {
        assert!(value, "{name}");
    }
    assert!(!options.webtoon);
}

#[test]
fn each_processing_switch_changes_only_its_corresponding_option() {
    let input = tempfile::tempdir().unwrap();
    let defaults = settings(input.path(), &[]);
    let defaults = pipeline_options(&defaults.cli, defaults.profile, defaults.output_format);
    for (flag, _) in processing_switches(&defaults) {
        let resolved = settings(input.path(), &[flag]);
        let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
        for (name, enabled) in processing_switches(&options) {
            assert_eq!(enabled, name == flag, "{flag} unexpectedly changed {name}");
        }
    }
}

#[test]
fn processing_enum_choices_are_translated_without_changing_their_meaning() {
    let input = tempfile::tempdir().unwrap();
    for (crop, cropping, split, splitter, ipc, inter_panel_crop) in [
        (
            "disabled",
            CroppingMode::Disabled,
            "split",
            SplitterMode::Split,
            "disabled",
            mangapress_core::crop::inter_panel::InterPanelMode::Disabled,
        ),
        (
            "margins",
            CroppingMode::Margins,
            "rotate",
            SplitterMode::Rotate,
            "horizontal",
            mangapress_core::crop::inter_panel::InterPanelMode::Horizontal,
        ),
        (
            "margins-and-page-numbers",
            CroppingMode::MarginsAndPageNumbers,
            "both",
            SplitterMode::Both,
            "both",
            mangapress_core::crop::inter_panel::InterPanelMode::Both,
        ),
    ] {
        let resolved = settings(
            input.path(),
            &["--cropping", crop, "--splitter", split, "--ipc", ipc],
        );
        let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
        assert_eq!(options.cropping, cropping);
        assert_eq!(options.splitter, splitter);
        assert_eq!(options.inter_panel_crop, inter_panel_crop);
    }
}

#[test]
fn webtoon_overrides_processing_options_without_rewriting_the_cli() {
    let input = tempfile::tempdir().unwrap();
    let resolved = settings(
        input.path(),
        &["--webtoon", "--manga-style", "--upscale", "--blackborders"],
    );
    let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
    assert!(options.webtoon && options.white_borders);
    assert!(!options.manga_style && !options.upscale && !options.black_borders);
    assert!(resolved.cli.manga_style && resolved.cli.upscale && resolved.cli.blackborders);
    assert!(!resolved.cli.whiteborders);
}

#[test]
fn default_optional_processing_choices_remain_unspecified() {
    let input = tempfile::tempdir().unwrap();
    let mut failure = initial_failure();
    let resolved = resolve(arguments(input.path(), &[]), &mut failure).unwrap();
    let options = pipeline_options(&resolved.cli, resolved.profile, resolved.output_format);
    assert!(options.width_override.is_none() && options.height_override.is_none());
    assert!(options.gamma.is_none() && options.jpeg_quality.is_none());
    assert_eq!(options.cropping, CroppingMode::MarginsAndPageNumbers);
    assert_eq!(options.splitter, SplitterMode::Split);
    assert_eq!(options.cropping_power, 1.0);
    assert_eq!(options.cropping_minimum, 0.0);
    assert_eq!(options.preserve_margin_percent, 0.0);
    for (flag, enabled) in processing_switches(&options) {
        assert!(!enabled, "default {flag}");
    }
    // Resolution does not reset the caller's last failure context on success.
    assert_eq!(failure.code, "unknown_profile");
}
