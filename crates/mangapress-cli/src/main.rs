mod args;

use anyhow::{bail, Context};
use args::{Cli, Cropping, Format, InterPanelCrop, Splitter};
use clap::Parser;
use mangapress_core::archive::{cbz::extract_cbz, folder::read_folder, SourceEntry};
use mangapress_core::ebook::{cbz_out, epub, group_into_chapters, Chapter, Page};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::pipeline::{
    process_page, CroppingMode, OutputFormat, PipelineOptions, SplitterMode,
};
use mangapress_core::profile::Profile;
use std::io::Write as _;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let profile = Profile::by_code(&cli.profile)
        .with_context(|| format!("unknown device profile '{}'", cli.profile))?;

    if !cli.input.exists() {
        bail!("input path does not exist: {}", cli.input.display());
    }

    let (width, height) = profile.effective_resolution(cli.customwidth, cli.customheight);
    if width == 0 || height == 0 {
        bail!(
            "resolved target resolution is {width}x{height} — profile '{}' has no built-in \
             resolution, pass both --customwidth and --customheight to set one",
            cli.profile
        );
    }

    if matches!(cli.format, Format::Pdf) {
        bail!("PDF output is not implemented yet (see docs/adr/ for what's built so far)");
    }

    let title = cli.title.clone().unwrap_or_else(|| {
        cli.input
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string())
    });
    let author = cli.author.clone().unwrap_or_else(|| "Unknown".to_string());

    println!(
        "mangapress: converting '{}' for {} ({width}x{height}, {} gray levels), manga_style={}, format={:?}",
        cli.input.display(),
        profile.display_name,
        profile.palette.levels(),
        cli.manga_style,
        cli.format,
    );

    let source_entries: Vec<SourceEntry> = if cli.input.is_dir() {
        read_folder(&cli.input)
    } else {
        extract_cbz(&cli.input)
    }
    .with_context(|| format!("reading input from {}", cli.input.display()))?;

    if source_entries.is_empty() {
        bail!("no files found in {}", cli.input.display());
    }

    let source_chapters = group_into_chapters(source_entries);
    let total_pages: usize = source_chapters.iter().map(|c| c.pages.len()).sum();
    println!(
        "found {} chapter(s), {total_pages} page(s) total",
        source_chapters.len()
    );

    let pipeline_options = PipelineOptions {
        profile,
        width_override: cli.customwidth,
        height_override: cli.customheight,
        manga_style: cli.manga_style,
        cropping: match cli.cropping {
            Cropping::Disabled => CroppingMode::Disabled,
            Cropping::Margins => CroppingMode::Margins,
            Cropping::MarginsAndPageNumbers => CroppingMode::MarginsAndPageNumbers,
        },
        cropping_power: 1.0,
        cropping_minimum: 0.0,
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
        upscale: cli.upscale,
        stretch: cli.stretch,
        wallpaper: cli.wallpaper,
        white_borders: cli.whiteborders,
        rotate_right: cli.rotateright,
        force_png: cli.forcepng,
        output_format: match cli.format {
            Format::Epub => OutputFormat::Epub,
            Format::Cbz => OutputFormat::Cbz,
            Format::Pdf => OutputFormat::Pdf,
        },
        gamma: cli.gamma,
        autolevel: cli.autolevel,
        noautocontrast: cli.noautocontrast,
        erase_rainbow: cli.eraserainbow,
    };

    let mut processed_chapters = Vec::with_capacity(source_chapters.len());
    let mut page_count = 0usize;
    for chapter in source_chapters {
        let mut pages = Vec::with_capacity(chapter.pages.len());
        for source_page in chapter.pages {
            let outputs = process_page(&source_page.bytes, &pipeline_options)
                .with_context(|| format!("processing a page in chapter '{}'", chapter.title))?;
            for (extension, bytes) in outputs {
                pages.push(Page { extension, bytes });
                page_count += 1;
            }
        }
        processed_chapters.push(Chapter {
            relative_path: chapter.relative_path,
            title: chapter.title,
            pages,
        });
        print!(".");
        std::io::stdout().flush().ok();
    }
    println!("\nprocessed {page_count} page(s)");

    let output_bytes = match cli.format {
        Format::Epub => epub::build_epub(
            &processed_chapters,
            &epub::EpubOptions {
                title: title.clone(),
                author,
                language: cli.language.clone(),
                reading_direction: ReadingDirection {
                    right_to_left: cli.manga_style,
                },
            },
        )?,
        Format::Cbz => cbz_out::build_cbz(&processed_chapters)?,
        Format::Pdf => unreachable!("checked above"),
    };

    let extension = match cli.format {
        Format::Epub => "epub",
        Format::Cbz => "cbz",
        Format::Pdf => "pdf",
    };
    let output_path = match cli.output {
        Some(path) if path.is_dir() => path.join(format!("{title}.{extension}")),
        Some(path) => path,
        None => cli.input.with_extension(extension),
    };

    std::fs::write(&output_path, &output_bytes)
        .with_context(|| format!("writing output to {}", output_path.display()))?;
    println!(
        "wrote {} ({} bytes)",
        output_path.display(),
        output_bytes.len()
    );

    Ok(())
}
