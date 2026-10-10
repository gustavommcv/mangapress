//! Resolve conversion inputs and translate CLI options into core processing options.

use crate::args::{
    automatic_format, pipeline_format, Cli, Cropping, Format, InterPanelCrop, Splitter,
};
use crate::protocol::RunFailure;
use crate::reporting::absolute_display;
use anyhow::{bail, Context};
use mangapress_core::pipeline::{CroppingMode, OutputFormat, PipelineOptions, SplitterMode};
use mangapress_core::profile::Profile;
use std::path::PathBuf;

/// Inputs accepted for conversion, before reading pages or emitting inspection events.
#[derive(Debug)]
pub(super) struct ResolvedConversion {
    pub(super) cli: Cli,
    pub(super) input: PathBuf,
    pub(super) input_path: String,
    pub(super) profile: &'static Profile,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) output_format: OutputFormat,
}

/// Preserve validation precedence; reporting-only modes bypass this conversion step.
pub(super) fn resolve(cli: Cli, failure: &mut RunFailure) -> anyhow::Result<ResolvedConversion> {
    // Guaranteed present: clap's `required_unless_present` on `--list-profiles`
    // means we only get here when `input` was actually passed.
    let input = cli
        .input
        .clone()
        .expect("input is required unless --list-profiles or --protocol-version");
    let input_path = absolute_display(&input);

    *failure = RunFailure::new(
        "unknown_profile",
        "configuration",
        true,
        format!("Unknown device profile '{}'.", cli.profile),
        match Profile::closest_code(&cli.profile) {
            Some(suggestion) => format!(
                "unknown device profile '{}' -- did you mean '{suggestion}'? (see --list-profiles)",
                cli.profile
            ),
            None => format!(
                "unknown device profile '{}' (see --list-profiles)",
                cli.profile
            ),
        },
    );
    let profile = Profile::by_code(&cli.profile).context(failure.diagnostic.clone())?;

    if !input.exists() {
        *failure = RunFailure::new(
            "input_not_found",
            "inspect",
            true,
            "The input path does not exist.",
            format!("input path does not exist: {}", input.display()),
        )
        .with_path(input_path.clone());
        bail!("input path does not exist: {}", input.display());
    }

    let mut cli = cli;
    if cli.format == Format::Auto {
        // Only an EPUB has a two-level table of contents (ADR 0012), so asking for one is
        // asking for an EPUB.
        cli.format = if cli.nested_toc {
            Format::Epub
        } else {
            automatic_format(profile)
        };
    }
    let cli = cli;
    let output_format = pipeline_format(cli.format);
    let (width, height) =
        output_format.target_resolution(profile, cli.customwidth, cli.customheight);
    if width == 0 || height == 0 {
        *failure = RunFailure::new(
            "invalid_resolution",
            "configuration",
            true,
            "Set both a target width and height for this device profile.",
            format!(
                "resolved target resolution is {width}x{height} — profile '{}' has no built-in resolution, pass both --customwidth and --customheight to set one",
                cli.profile
            ),
        );
        bail!(
            "resolved target resolution is {width}x{height} — profile '{}' has no built-in \
             resolution, pass both --customwidth and --customheight to set one",
            cli.profile
        );
    }

    if cli.nested_toc && cli.format != Format::Epub {
        *failure = RunFailure::new(
            "nested_toc_unsupported_format",
            "configuration",
            true,
            "A two-level table of contents is only available for EPUB output right now.",
            format!(
                "--nested-toc was combined with --format {:?}, which has no chapter/volume table of \
                 contents mechanism yet — see docs/adr/0012-nested-toc-for-combined-volumes.md",
                cli.format
            ),
        );
        bail!(
            "--nested-toc requires --format epub, got --format {:?}",
            cli.format
        );
    }

    Ok(ResolvedConversion {
        cli,
        input,
        input_path,
        profile,
        width,
        height,
        output_format,
    })
}

pub(super) fn pipeline_options(
    cli: &Cli,
    profile: &'static Profile,
    output_format: OutputFormat,
) -> PipelineOptions {
    PipelineOptions {
        profile,
        width_override: cli.customwidth,
        height_override: cli.customheight,
        // Upstream's webtoon mode forces these four whatever was asked for.
        manga_style: cli.manga_style && !cli.webtoon,
        cropping: match cli.cropping {
            Cropping::Disabled => CroppingMode::Disabled,
            Cropping::Margins => CroppingMode::Margins,
            Cropping::MarginsAndPageNumbers => CroppingMode::MarginsAndPageNumbers,
        },
        cropping_power: cli.croppingpower,
        cropping_minimum: cli.croppingminimum,
        preserve_margin_percent: cli.preservemargin,
        inter_panel_crop: match cli.interpanelcrop {
            InterPanelCrop::Disabled => {
                mangapress_core::crop::inter_panel::InterPanelMode::Disabled
            }
            InterPanelCrop::Horizontal => {
                mangapress_core::crop::inter_panel::InterPanelMode::Horizontal
            }
            InterPanelCrop::Both => mangapress_core::crop::inter_panel::InterPanelMode::Both,
        },
        splitter: match cli.splitter {
            Splitter::Split => SplitterMode::Split,
            Splitter::Rotate => SplitterMode::Rotate,
            Splitter::Both => SplitterMode::Both,
        },
        upscale: cli.upscale && !cli.webtoon,
        stretch: cli.stretch,
        wallpaper: cli.wallpaper,
        white_borders: cli.whiteborders || cli.webtoon,
        black_borders: cli.blackborders && !cli.webtoon,
        webtoon: cli.webtoon,
        no_rotate: cli.norotate,
        rotate_first: cli.rotatefirst,
        maximize_strips: cli.maximizestrips,
        color_autocontrast: cli.colorautocontrast,
        force_color: cli.forcecolor,
        force_png_rgb: cli.force_png_rgb,
        png_legacy: cli.pnglegacy,
        no_quantize: cli.noquantize,
        no_processing: cli.noprocessing,
        rotate_right: cli.rotateright,
        force_png: cli.forcepng,
        output_format,
        gamma: cli.gamma,
        autolevel: cli.autolevel,
        noautocontrast: cli.noautocontrast,
        erase_rainbow: cli.eraserainbow,
        jpeg_quality: cli.jpeg_quality,
    }
}

#[cfg(test)]
mod tests;
