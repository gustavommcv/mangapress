mod args;

use anyhow::{bail, Context};
use args::{Cli, Cropping, Format, InterPanelCrop, MetadataTitle, Splitter};
use clap::Parser;
use mangapress_core::archive::{cbz::extract_cbz, folder::read_folder, SourceEntry};
use mangapress_core::ebook::{cbz_out, epub, group_into_chapters, pdf, Chapter, Page};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::metadata::{self, MetadataTitleMode};
use mangapress_core::pipeline::{
    process_page, CroppingMode, OutputFormat, PipelineOptions, SplitterMode,
};
use mangapress_core::profile::Profile;
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};

/// A resolved book title (from `--title`, `ComicInfo.xml`'s `Series`/`Title`,
/// or the input filename) can contain characters that are illegal in a
/// filename on Windows, or that `/`/`\` would misread as path separators on
/// any platform (e.g. `--metadatatitle combine` appends `": Title"`) —
/// replace the reserved set with `-` before using the title as an output
/// filename.
fn sanitize_filename(name: &str) -> String {
    name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "-")
}

/// Best-effort "do these two paths refer to the same file" check. `input`
/// always exists by this point; `candidate_output` usually doesn't yet, so
/// this can't just canonicalize both and compare -- it canonicalizes
/// `candidate_output`'s parent instead and rejoins the file name. Good
/// enough to catch the case this exists for (an unset `--output`, or an
/// explicit one, that resolves to the same file being read) without a new
/// dependency; it isn't a substitute for a real same-file check across
/// hardlinks etc.
fn same_file(input: &Path, candidate_output: &Path) -> bool {
    fn resolve(path: &Path) -> Option<PathBuf> {
        if path.exists() {
            return std::fs::canonicalize(path).ok();
        }
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty())?;
        let file_name = path.file_name()?;
        Some(std::fs::canonicalize(parent).ok()?.join(file_name))
    }

    match (resolve(input), resolve(candidate_output)) {
        (Some(a), Some(b)) => a == b,
        _ => input == candidate_output,
    }
}

/// A sibling path that doesn't collide with `path`, for when `path` would
/// otherwise overwrite the very input it was derived from. Mirrors KCC's
/// own `getOutputFilename()`, which appends a `_kccN` suffix for the same
/// reason: converting a `.cbz` back to `.cbz` with no explicit `--output`
/// would otherwise destroy the source file being read.
fn disambiguate_output_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path.extension().map(|e| e.to_string_lossy().into_owned());
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());

    for n in 1.. {
        let name = if n == 1 {
            format!("{stem} (mangapress)")
        } else {
            format!("{stem} (mangapress {n})")
        };
        let candidate_name = match &extension {
            Some(ext) => format!("{name}.{ext}"),
            None => name,
        };
        let candidate = match parent {
            Some(dir) => dir.join(candidate_name),
            None => PathBuf::from(candidate_name),
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.list_profiles {
        // `writeln!` + break-on-error, not `println!`, because `println!`
        // panics on a write failure -- including the very ordinary
        // `mangapress --list-profiles | head` closing its end of the pipe
        // early. Piping a listing into `head`/`grep`/`less` should just
        // stop quietly, like it does for any real Unix tool.
        let mut stdout = std::io::stdout().lock();
        for p in mangapress_core::profile::PROFILES {
            if writeln!(
                stdout,
                "{:<10} {:<40} {}x{}",
                p.code, p.display_name, p.width, p.height
            )
            .is_err()
            {
                break;
            }
        }
        return Ok(());
    }
    let quiet = cli.quiet;
    // Guaranteed present: clap's `required_unless_present` on `--list-profiles`
    // means we only get here when `input` was actually passed.
    let input = cli.input.expect("input is required unless --list-profiles");

    let profile = Profile::by_code(&cli.profile).with_context(|| {
        match Profile::closest_code(&cli.profile) {
            Some(suggestion) => format!(
                "unknown device profile '{}' -- did you mean '{suggestion}'? (see --list-profiles)",
                cli.profile
            ),
            None => format!(
                "unknown device profile '{}' (see --list-profiles)",
                cli.profile
            ),
        }
    })?;

    if !input.exists() {
        bail!("input path does not exist: {}", input.display());
    }

    let (width, height) = profile.effective_resolution(cli.customwidth, cli.customheight);
    if width == 0 || height == 0 {
        bail!(
            "resolved target resolution is {width}x{height} — profile '{}' has no built-in \
             resolution, pass both --customwidth and --customheight to set one",
            cli.profile
        );
    }

    let fallback_title = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    if !quiet {
        eprintln!(
            "mangapress: converting '{}' for {} ({width}x{height}, {} gray levels), manga_style={}, format={:?}",
            input.display(),
            profile.display_name,
            profile.palette.levels(),
            cli.manga_style,
            cli.format,
        );
    }

    let mut source_entries: Vec<SourceEntry> = if input.is_dir() {
        read_folder(&input)
    } else {
        extract_cbz(&input)
    }
    .with_context(|| format!("reading input from {}", input.display()))?;

    if source_entries.is_empty() {
        bail!("no files found in {}", input.display());
    }

    let comic_info_xml = metadata::extract_comic_info_entry(&mut source_entries);
    let comic_info = comic_info_xml
        .as_deref()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .map(|xml| metadata::parse_comic_info_xml(&xml))
        .transpose()
        .with_context(|| format!("parsing ComicInfo.xml from {}", input.display()))?;
    if comic_info.is_some() && !quiet {
        eprintln!("found ComicInfo.xml");
    }

    let (source_entries, skipped_non_images) =
        mangapress_core::archive::filter_image_entries(source_entries);
    if skipped_non_images > 0 {
        eprintln!(
            "warning: skipped {skipped_non_images} non-image file(s) in the input (not a \
             recognized image extension)"
        );
    }
    if source_entries.is_empty() {
        bail!("no recognized page images found in {}", input.display());
    }

    let resolved = metadata::resolve(
        comic_info.as_ref(),
        cli.title.as_deref(),
        cli.author.as_deref(),
        &fallback_title,
        match cli.metadatatitle {
            MetadataTitle::SeriesOnly => MetadataTitleMode::SeriesOnly,
            MetadataTitle::Combine => MetadataTitleMode::Combine,
            MetadataTitle::TitleOnly => MetadataTitleMode::TitleOnly,
        },
    );
    let title = resolved.title;
    let author = resolved.authors.join(", ");

    let source_chapters = group_into_chapters(source_entries);
    let total_chapters = source_chapters.len();
    let total_pages: usize = source_chapters.iter().map(|c| c.pages.len()).sum();
    if !quiet {
        eprintln!("found {total_chapters} chapter(s), {total_pages} page(s) total");
    }

    let extension = match cli.format {
        Format::Epub => "epub",
        Format::Cbz => "cbz",
        Format::Pdf => "pdf",
    };
    let output_path = match &cli.output {
        Some(path) if path.is_dir() => {
            path.join(format!("{}.{extension}", sanitize_filename(&title)))
        }
        Some(path) => path.clone(),
        None => input.with_extension(extension),
    };
    // With no --output (or an explicit one matching the input), converting
    // a `.cbz` back to `.cbz` would otherwise silently overwrite the very
    // source file being read.
    let output_path = if same_file(&input, &output_path) {
        let disambiguated = disambiguate_output_path(&output_path);
        eprintln!(
            "warning: output would overwrite the input file; writing to {} instead",
            disambiguated.display()
        );
        disambiguated
    } else {
        output_path
    };

    if cli.dry_run {
        println!("dry run -- no pages will be processed, nothing will be written");
        println!("title: {title}");
        println!("author: {author}");
        println!("format: {:?}", cli.format);
        println!("device: {} ({width}x{height})", profile.display_name);
        println!("chapters: {total_chapters}, pages: {total_pages}");
        println!("would write: {}", output_path.display());
        return Ok(());
    }

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
        jpeg_quality: cli.jpeg_quality,
    };

    // Interactive terminals get a live per-page counter (overwritten in
    // place via `\r`); redirected/piped output gets one plain line per
    // chapter instead, so a log file doesn't fill up with carriage returns.
    let progress_is_tty = std::io::stderr().is_terminal();

    let mut processed_chapters = Vec::with_capacity(total_chapters);
    let mut page_count = 0usize;
    let mut source_page_count = 0usize;
    for (chapter_index, chapter) in source_chapters.into_iter().enumerate() {
        let chapter_title = chapter.title.clone();
        let mut pages = Vec::with_capacity(chapter.pages.len());
        for source_page in chapter.pages {
            let outputs = process_page(&source_page.bytes, &pipeline_options)
                .with_context(|| format!("processing a page in chapter '{chapter_title}'"))?;
            for (extension, bytes) in outputs {
                pages.push(Page { extension, bytes });
                page_count += 1;
            }
            source_page_count += 1;
            if !quiet && progress_is_tty {
                eprint!("\rprocessing page {source_page_count}/{total_pages}");
                std::io::stderr().flush().ok();
            }
        }
        processed_chapters.push(Chapter {
            relative_path: chapter.relative_path,
            title: chapter.title,
            pages,
        });
        if !quiet && !progress_is_tty {
            eprintln!(
                "chapter {}/{total_chapters} done ({page_count}/{total_pages} pages so far)",
                chapter_index + 1
            );
        }
    }
    if !quiet {
        if progress_is_tty {
            eprintln!();
        }
        eprintln!("processed {page_count} page(s)");
    }

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
                description: resolved.summary,
            },
        )?,
        Format::Cbz => {
            let keep_xml = cli
                .keepcomicinfo
                .then_some(comic_info_xml.as_deref())
                .flatten();
            cbz_out::build_cbz(&processed_chapters, keep_xml)?
        }
        Format::Pdf => pdf::build_pdf(
            &processed_chapters,
            &pdf::PdfOptions {
                title: title.clone(),
                author: author.clone(),
            },
        )?,
    };

    std::fs::write(&output_path, &output_bytes)
        .with_context(|| format!("writing output to {}", output_path.display()))?;
    if !quiet {
        eprintln!(
            "wrote {} ({} bytes)",
            output_path.display(),
            output_bytes.len()
        );
    }

    Ok(())
}
