mod args;
mod protocol;

use anyhow::{bail, Context};
use args::{Cli, Cropping, Format, InterPanelCrop, MetadataTitle, Splitter};
use clap::{error::ErrorKind, Parser};
use mangapress_core::archive::{cbz::extract_cbz, folder::read_folder, SourceEntry};
use mangapress_core::ebook::{cbz_out, epub, group_into_chapters, pdf, Chapter, Page};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::metadata::{self, MetadataTitleMode};
use mangapress_core::pipeline::{
    process_page, CroppingMode, OutputFormat, PipelineOptions, SplitterMode,
};
use mangapress_core::profile::Profile;
use protocol::{event_write_failure, EventSink, RunFailure};
use rayon::prelude::*;
use serde_json::json;
use std::ffi::OsString;
use std::io::{IsTerminal, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

/// Does `path` already look like a path to one of the ebook formats this
/// tool writes? Used to decide, for an `--output` path that doesn't exist
/// yet, whether it names an output *file* to create (so it's a literal
/// path) or an output *directory* to create (so a filename still needs to
/// be derived from the title) -- an `--output` naming a not-yet-created
/// directory would otherwise silently produce an extension-less file
/// literally named after that directory.
fn has_known_output_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("epub") | Some("cbz") | Some("pdf")
    )
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

fn absolute_display(path: &Path) -> String {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
    .to_string_lossy()
    .into_owned()
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::Epub => "epub",
        Format::Cbz => "cbz",
        Format::Pdf => "pdf",
    }
}

/// Processes every page of one chapter, fanning the work out across every
/// core (pages within a chapter don't depend on each other) while
/// preserving input order in the returned `Vec` regardless of which thread
/// finishes first -- `par_iter().map().collect()` guarantees this. Kept
/// directly testable: `on_page_done` is the only way callers observe
/// progress. Callbacks are serialized in stable source-page order even when
/// workers finish out of order; both arguments are one-based source-page
/// positions (not output pages -- one source page can expand into more than
/// one, via a double-page-spread split).
#[derive(Debug)]
struct PageProcessingFailure {
    page: usize,
    diagnostic: String,
}

fn process_chapter_pages(
    pages: &[Page],
    options: &PipelineOptions,
    on_page_done: impl Fn(usize, usize) -> std::io::Result<()> + Sync,
) -> Result<Vec<Page>, PageProcessingFailure> {
    let progress = Mutex::new((vec![false; pages.len()], 0usize));
    let outputs: Vec<Vec<(String, Vec<u8>)>> = pages
        .par_iter()
        .enumerate()
        .map(|(page_index, source_page)| {
            let page_number = page_index + 1;
            let result = process_page(&source_page.bytes, options).map_err(|error| {
                PageProcessingFailure {
                    page: page_number,
                    diagnostic: error.to_string(),
                }
            })?;
            let mut progress = progress.lock().map_err(|_| PageProcessingFailure {
                page: page_number,
                diagnostic: "page progress lock was poisoned".to_string(),
            })?;
            progress.0[page_index] = true;
            while progress.1 < progress.0.len() && progress.0[progress.1] {
                progress.1 += 1;
                let completed = progress.1;
                on_page_done(completed, completed).map_err(|error| PageProcessingFailure {
                    page: completed,
                    diagnostic: format!("writing page progress: {error}"),
                })?;
            }
            Ok(result)
        })
        .collect::<Result<Vec<_>, PageProcessingFailure>>()?;

    let mut flattened = Vec::with_capacity(pages.len());
    for page_outputs in outputs {
        for (extension, bytes) in page_outputs {
            flattened.push(Page { extension, bytes });
        }
    }
    Ok(flattened)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let machine_requested = args
        .iter()
        .any(|arg| arg == "--json-events" || arg == "--protocol-version");
    let events = EventSink::new(machine_requested, std::io::stdout());
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                error.exit();
            }
            if machine_requested {
                let failure = RunFailure::new(
                    "invalid_arguments",
                    "configuration",
                    true,
                    "The command-line arguments are invalid.",
                    error.to_string(),
                );
                events.emit_failure(&failure).map_err(event_write_failure)?;
            }
            error.exit();
        }
    };

    if cli.protocol_version {
        events
            .emit(
                "protocol",
                json!({
                    "capabilities": ["events", "profiles"],
                }),
            )
            .map_err(event_write_failure)?;
        return Ok(());
    }

    let mut failure = RunFailure::new(
        "conversion_failed",
        "conversion",
        false,
        "The conversion couldn't be completed.",
        "conversion failed",
    );
    let result = run(cli, &events, &mut failure);
    if let Err(error) = &result {
        if let Some(protocol_failure) = error.downcast_ref::<RunFailure>() {
            failure = protocol_failure.clone();
        } else {
            failure.diagnostic = format!("{error:#}");
        }
        if events.enabled() {
            events.emit_failure(&failure).map_err(event_write_failure)?;
        }
    }
    result
}

fn run<W: std::io::Write + Send>(
    cli: Cli,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<()> {
    if cli.list_profiles {
        // `writeln!` + break-on-error, not `println!`, because `println!`
        // panics on a write failure -- including the very ordinary
        // `mangapress --list-profiles | head` closing its end of the pipe
        // early. Piping a listing into `head`/`grep`/`less` should just
        // stop quietly, like it does for any real Unix tool.
        if events.enabled() {
            for profile in mangapress_core::profile::PROFILES {
                events
                    .emit(
                        "profile",
                        json!({
                            "code": profile.code,
                            "name": profile.display_name,
                            "width": profile.width,
                            "height": profile.height,
                            "gray_levels": profile.palette.levels(),
                            "family": format!("{:?}", profile.family()).to_ascii_lowercase(),
                        }),
                    )
                    .map_err(event_write_failure)?;
            }
            events
                .emit(
                    "result",
                    json!({
                        "status": "completed",
                        "operation": "list_profiles",
                        "profile_count": mangapress_core::profile::PROFILES.len(),
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
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
        }
        return Ok(());
    }
    let quiet = cli.quiet || events.enabled();
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

    let (width, height) = profile.effective_resolution(cli.customwidth, cli.customheight);
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

    let fallback_title = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    events
        .emit(
            "stage",
            json!({
                "stage": "inspect",
                "state": "started",
                "path": input_path.clone(),
            }),
        )
        .map_err(event_write_failure)?;

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

    *failure = RunFailure::new(
        "input_read_failed",
        "inspect",
        true,
        "Couldn't read pages from the input.",
        format!("reading input from {}", input.display()),
    )
    .with_path(input_path.clone());
    let mut source_entries: Vec<SourceEntry> = if input.is_dir() {
        read_folder(&input)
    } else {
        extract_cbz(&input)
    }
    .with_context(|| format!("reading input from {}", input.display()))?;

    if source_entries.is_empty() {
        *failure = RunFailure::new(
            "input_empty",
            "inspect",
            true,
            "The input contains no files.",
            format!("no files found in {}", input.display()),
        )
        .with_path(input_path.clone());
        bail!("no files found in {}", input.display());
    }

    events
        .emit(
            "stage",
            json!({
                "stage": "metadata",
                "state": "started",
                "path": input_path.clone(),
            }),
        )
        .map_err(event_write_failure)?;
    let comic_info_xml = metadata::extract_comic_info_entry(&mut source_entries);
    *failure = RunFailure::new(
        "metadata_parse_failed",
        "metadata",
        true,
        "Couldn't read ComicInfo.xml metadata from the input.",
        format!("parsing ComicInfo.xml from {}", input.display()),
    )
    .with_path(input_path.clone());
    let comic_info = comic_info_xml
        .as_deref()
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .map(|xml| metadata::parse_comic_info_xml(&xml))
        .transpose()
        .with_context(|| format!("parsing ComicInfo.xml from {}", input.display()))?;
    if comic_info.is_some() && !quiet {
        eprintln!("found ComicInfo.xml");
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
    events
        .emit(
            "stage",
            json!({
                "stage": "metadata",
                "state": "completed",
                "manga": title.clone(),
                "title": title.clone(),
                "author": author.clone(),
                "comic_info_found": comic_info.is_some(),
            }),
        )
        .map_err(event_write_failure)?;

    let (source_entries, skipped_non_images) =
        mangapress_core::archive::filter_image_entries(source_entries);
    if skipped_non_images > 0 {
        let message = format!(
            "Skipped {skipped_non_images} non-image file(s) because their extensions aren't recognized."
        );
        if events.enabled() {
            events
                .emit(
                    "warning",
                    json!({
                        "severity": "warning",
                        "code": "skipped_non_images",
                        "stage": "inspect",
                        "path": input_path.clone(),
                        "recoverable": true,
                        "message": message,
                        "count": skipped_non_images,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!(
                "warning: skipped {skipped_non_images} non-image file(s) in the input (not a \
                 recognized image extension)"
            );
        }
    }
    if source_entries.is_empty() {
        *failure = RunFailure::new(
            "no_page_images",
            "inspect",
            true,
            "The input contains no recognized page images.",
            format!("no recognized page images found in {}", input.display()),
        )
        .with_path(input_path.clone());
        bail!("no recognized page images found in {}", input.display());
    }

    let source_chapters = group_into_chapters(source_entries);
    let total_chapters = source_chapters.len();
    let total_pages: usize = source_chapters.iter().map(|c| c.pages.len()).sum();
    if !quiet {
        eprintln!("found {total_chapters} chapter(s), {total_pages} page(s) total");
    }
    events
        .emit(
            "stage",
            json!({
                "stage": "inspect",
                "state": "completed",
                "manga": title.clone(),
                "chapters": total_chapters,
                "pages": total_pages,
            }),
        )
        .map_err(event_write_failure)?;

    events
        .emit(
            "stage",
            json!({
                "stage": "plan",
                "state": "started",
                "manga": title.clone(),
            }),
        )
        .map_err(event_write_failure)?;
    let extension = format_name(cli.format);
    let output_path = match &cli.output {
        Some(path) if path.is_dir() => {
            path.join(format!("{}.{extension}", sanitize_filename(&title)))
        }
        Some(path) if !path.exists() && !has_known_output_extension(path) => {
            // `--dry-run` promises nothing gets written -- creating the
            // directory here would itself be a side effect, so only the
            // hypothetical path is computed; the real create happens below,
            // once dry-run has already returned.
            if !cli.dry_run {
                *failure = RunFailure::new(
                    "output_directory_create_failed",
                    "write",
                    true,
                    "Couldn't create the selected output folder.",
                    format!("creating output directory {}", path.display()),
                )
                .with_manga(title.clone())
                .with_path(absolute_display(path));
                std::fs::create_dir_all(path)
                    .with_context(|| format!("creating output directory {}", path.display()))?;
            }
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
        if events.enabled() {
            events
                .emit(
                    "warning",
                    json!({
                        "severity": "warning",
                        "code": "output_collision",
                        "stage": "plan",
                        "manga": title.clone(),
                        "path": absolute_display(&disambiguated),
                        "recoverable": true,
                        "message": "The requested output would overwrite the input, so a safe alternate filename will be used.",
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!(
                "warning: output would overwrite the input file; writing to {} instead",
                disambiguated.display()
            );
        }
        disambiguated
    } else {
        output_path
    };
    let output_path_absolute = absolute_display(&output_path);
    events
        .emit(
            "stage",
            json!({
                "stage": "plan",
                "state": "completed",
                "manga": title.clone(),
                "output_path": output_path_absolute.clone(),
                "format": extension,
                "profile": profile.code,
                "device": profile.display_name,
                "width": width,
                "height": height,
                "gray_levels": profile.palette.levels(),
                "chapters": total_chapters,
                "pages": total_pages,
            }),
        )
        .map_err(event_write_failure)?;

    if cli.dry_run {
        if events.enabled() {
            events
                .emit(
                    "result",
                    json!({
                        "status": "completed",
                        "operation": "convert",
                        "dry_run": true,
                        "manga": title,
                        "author": author,
                        "format": extension,
                        "profile": profile.code,
                        "width": width,
                        "height": height,
                        "chapters": total_chapters,
                        "source_pages": total_pages,
                        "output_path": output_path_absolute,
                        "written": false,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            println!("dry run -- no pages will be processed, nothing will be written");
            println!("title: {title}");
            println!("author: {author}");
            println!("format: {:?}", cli.format);
            println!("device: {} ({width}x{height})", profile.display_name);
            println!("chapters: {total_chapters}, pages: {total_pages}");
            println!("would write: {}", output_path.display());
        }
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
    let progress_is_tty = !events.enabled() && std::io::stderr().is_terminal();
    events
        .emit(
            "stage",
            json!({
                "stage": "process",
                "state": "started",
                "manga": title.clone(),
                "chapters": total_chapters,
                "pages": total_pages,
            }),
        )
        .map_err(event_write_failure)?;

    // Pages within a chapter are independent of each other -- nothing about
    // processing one depends on another -- so they're fanned out across
    // every available core via `process_chapter_pages` rather than one at a
    // time, matching upstream KCC's own `multiprocessing.Pool()`-based
    // fan-out, while still preserving page order in the output.
    let mut processed_chapters = Vec::with_capacity(total_chapters);
    let mut page_count = 0usize;
    let mut source_pages_done = 0usize;
    for (chapter_index, chapter) in source_chapters.into_iter().enumerate() {
        let chapter_title = chapter.title.clone();
        let chapter_source_len = chapter.pages.len();
        events
            .emit(
                "chapter",
                json!({
                    "state": "started",
                    "stage": "process",
                    "manga": title.clone(),
                    "chapter": chapter_title.clone(),
                    "chapter_index": chapter_index + 1,
                    "chapter_count": total_chapters,
                    "source_pages": chapter_source_len,
                }),
            )
            .map_err(event_write_failure)?;
        let pages = match process_chapter_pages(
            &chapter.pages,
            &pipeline_options,
            |done_in_chapter, page_number| {
                let done = source_pages_done + done_in_chapter;
                if events.enabled() {
                    events.emit(
                        "page",
                        json!({
                            "state": "completed",
                            "stage": "process",
                            "manga": title.clone(),
                            "chapter": chapter_title.clone(),
                            "chapter_index": chapter_index + 1,
                            "page": page_number,
                            "completed": done,
                            "total": total_pages,
                        }),
                    )?;
                } else if !quiet && progress_is_tty {
                    eprint!("\rprocessing page {done}/{total_pages}");
                    std::io::stderr().flush().ok();
                }
                Ok(())
            },
        ) {
            Ok(pages) => pages,
            Err(error) => {
                *failure = RunFailure::new(
                    "page_processing_failed",
                    "process",
                    true,
                    format!(
                        "Couldn't process page {} in chapter '{}'.",
                        error.page, chapter_title
                    ),
                    format!(
                        "processing page {} in chapter '{}': {}",
                        error.page, chapter_title, error.diagnostic
                    ),
                )
                .with_manga(title.clone())
                .with_chapter(chapter_title.clone())
                .with_page(error.page)
                .with_path(input_path.clone());
                bail!(
                    "processing a page in chapter '{chapter_title}': {}",
                    error.diagnostic
                );
            }
        };

        page_count += pages.len();
        source_pages_done += chapter_source_len;
        events
            .emit(
                "chapter",
                json!({
                    "state": "completed",
                    "stage": "process",
                    "manga": title.clone(),
                    "chapter": chapter_title,
                    "chapter_index": chapter_index + 1,
                    "chapter_count": total_chapters,
                    "source_pages": chapter_source_len,
                    "output_pages": pages.len(),
                    "completed": source_pages_done,
                    "total": total_pages,
                }),
            )
            .map_err(event_write_failure)?;
        processed_chapters.push(Chapter {
            relative_path: chapter.relative_path,
            title: chapter.title,
            pages,
        });
        if !quiet && !progress_is_tty {
            eprintln!(
                "chapter {}/{total_chapters} done ({source_pages_done}/{total_pages} pages so far)",
                chapter_index + 1,
            );
        }
    }
    if !quiet {
        if progress_is_tty {
            eprintln!();
        }
        eprintln!("processed {page_count} page(s)");
    }
    events
        .emit(
            "stage",
            json!({
                "stage": "process",
                "state": "completed",
                "manga": title.clone(),
                "source_pages": total_pages,
                "output_pages": page_count,
            }),
        )
        .map_err(event_write_failure)?;

    events
        .emit(
            "stage",
            json!({
                "stage": "package",
                "state": "started",
                "manga": title.clone(),
                "format": extension,
            }),
        )
        .map_err(event_write_failure)?;
    *failure = RunFailure::new(
        "book_build_failed",
        "package",
        false,
        format!(
            "Couldn't assemble the {} book.",
            extension.to_ascii_uppercase()
        ),
        format!("building {extension} output"),
    )
    .with_manga(title.clone())
    .with_path(output_path_absolute.clone());
    let result_author = author.clone();
    let output_result = match cli.format {
        Format::Epub => epub::build_epub(
            &processed_chapters,
            &epub::EpubOptions {
                title: title.clone(),
                author: author.clone(),
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
    let output_bytes = output_result;
    events
        .emit(
            "stage",
            json!({
                "stage": "package",
                "state": "completed",
                "manga": title.clone(),
                "format": extension,
                "bytes": output_bytes.len(),
            }),
        )
        .map_err(event_write_failure)?;

    *failure = RunFailure::new(
        "output_write_failed",
        "write",
        true,
        "Couldn't save the converted book.",
        format!("writing output to {}", output_path.display()),
    )
    .with_manga(title.clone())
    .with_path(output_path_absolute.clone());
    events
        .emit(
            "stage",
            json!({
                "stage": "write",
                "state": "started",
                "manga": title.clone(),
                "path": output_path_absolute.clone(),
            }),
        )
        .map_err(event_write_failure)?;
    std::fs::write(&output_path, &output_bytes)
        .with_context(|| format!("writing output to {}", output_path.display()))?;
    if !quiet {
        eprintln!(
            "wrote {} ({} bytes)",
            output_path.display(),
            output_bytes.len()
        );
    }
    events
        .emit(
            "stage",
            json!({
                "stage": "write",
                "state": "completed",
                "manga": title.clone(),
                "path": output_path_absolute.clone(),
                "bytes": output_bytes.len(),
            }),
        )
        .map_err(event_write_failure)?;
    events
        .emit(
            "result",
            json!({
                "status": "completed",
                "operation": "convert",
                "dry_run": false,
                "manga": title,
                "author": result_author,
                "format": extension,
                "profile": profile.code,
                "width": width,
                "height": height,
                "chapters": total_chapters,
                "source_pages": total_pages,
                "output_pages": page_count,
                "output_path": output_path_absolute,
                "bytes": output_bytes.len(),
                "written": true,
            }),
        )
        .map_err(event_write_failure)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_filename_replaces_every_reserved_character() {
        assert_eq!(
            sanitize_filename(r#"a/b\c:d*e?f"g<h>i|j"#),
            "a-b-c-d-e-f-g-h-i-j"
        );
    }

    #[test]
    fn sanitize_filename_leaves_ordinary_titles_alone() {
        assert_eq!(
            sanitize_filename("Chainsaw Man - Vol.01"),
            "Chainsaw Man - Vol.01"
        );
    }

    #[test]
    fn same_file_recognizes_an_existing_file_under_a_different_spelling() {
        let dir =
            std::env::temp_dir().join(format!("mangapress-samefile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("book.cbz");
        std::fs::write(&input, b"x").unwrap();

        // A not-yet-created candidate that resolves to the exact same path.
        let candidate = dir.join(".").join("book.cbz");
        assert!(same_file(&input, &candidate));

        let different = dir.join("other.cbz");
        assert!(!same_file(&input, &different));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn has_known_output_extension_recognizes_only_the_formats_this_tool_writes() {
        assert!(has_known_output_extension(Path::new("book.epub")));
        assert!(has_known_output_extension(Path::new("book.EPUB")));
        assert!(has_known_output_extension(Path::new("book.cbz")));
        assert!(has_known_output_extension(Path::new("book.pdf")));
        assert!(!has_known_output_extension(Path::new("book.zip")));
        assert!(!has_known_output_extension(Path::new("output-folder")));
    }

    #[test]
    fn disambiguate_output_path_appends_a_suffix_and_keeps_the_extension() {
        let dir = std::env::temp_dir().join(format!(
            "mangapress-disambiguate-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let colliding = dir.join("book.cbz");
        std::fs::write(&colliding, b"x").unwrap();

        let result = disambiguate_output_path(&colliding);
        assert_eq!(result, dir.join("book (mangapress).cbz"));
        assert!(!result.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn disambiguate_output_path_counts_up_when_the_first_suffix_also_collides() {
        let dir = std::env::temp_dir().join(format!(
            "mangapress-disambiguate-counting-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let colliding = dir.join("book.cbz");
        std::fs::write(&colliding, b"x").unwrap();
        std::fs::write(dir.join("book (mangapress).cbz"), b"x").unwrap();

        let result = disambiguate_output_path(&colliding);
        assert_eq!(result, dir.join("book (mangapress 2).cbz"));

        std::fs::remove_dir_all(&dir).ok();
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
            .map(|&g| Page {
                extension: "png".to_string(),
                bytes: solid_gray_png(40, g),
            })
            .collect();

        let options = minimal_pipeline_options();
        let processed = process_chapter_pages(&pages, &options, |_, _| Ok(())).unwrap();
        assert_eq!(processed.len(), gray_levels.len());

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
            })
            .collect();
        let options = minimal_pipeline_options();

        let calls = std::sync::Mutex::new(Vec::new());
        process_chapter_pages(&pages, &options, |done, page| {
            calls.lock().unwrap().push((done, page));
            Ok(())
        })
        .unwrap();

        assert_eq!(
            calls.into_inner().unwrap(),
            vec![(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]
        );
    }
}
