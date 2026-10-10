mod args;
mod discovery;
mod output;
mod processing;
mod protocol;
mod reporting;

use anyhow::{bail, Context};
use args::{
    automatic_format, format_name, pipeline_format, Cli, Cropping, Format, InterPanelCrop,
    MetadataTitle, Splitter,
};
use clap::{error::ErrorKind, Parser};
#[cfg(test)]
use discovery::COVERS_FOLDER;
use discovery::{cover_by_convention, read_spread_labels, spread_labels_beside};
use mangapress_core::archive::read_book;
use mangapress_core::ebook::{
    cbz_out, cover, epub, group_into_chapters, pdf, spreads, Chapter, Page,
};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::metadata::{self, MetadataTitleMode};
#[cfg(test)]
use mangapress_core::pipeline::OutputFormat;
use mangapress_core::pipeline::{
    effective_default_jpeg_quality, effective_palette, CroppingMode, PipelineOptions, SplitterMode,
};
use mangapress_core::profile::{Family, Profile};
use processing::process_chapter_pages;
use protocol::{event_write_failure, EventSink, RunFailure};
use reporting::{absolute_display, utc_timestamp, warn, write_human_report};
use serde_json::json;
use std::ffi::OsString;
use std::io::{IsTerminal, Write as _};
#[cfg(test)]
use std::path::PathBuf;

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
                    "capabilities": ["events", "profiles", "nested_toc"],
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
            write_human_report(&mut std::io::stdout().lock(), |stdout| {
                for p in mangapress_core::profile::PROFILES {
                    writeln!(
                        stdout,
                        "{:<10} {:<40} {}x{}",
                        p.code, p.display_name, p.width, p.height
                    )?;
                }
                Ok(())
            })
            .context("writing the profile list")?;
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

    let mut cli = cli;
    if cli.format == Format::Auto {
        cli.format = automatic_format(profile);
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
            effective_palette(profile, cli.customwidth, cli.customheight).levels(),
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
    let book_input =
        read_book(&input).with_context(|| format!("reading input from {}", input.display()))?;
    for link in &book_input.skipped_links {
        let path = absolute_display(&input.join(&link.relative_path));
        let message = format!("Skipped a symbolic link because {}.", link.reason);
        if events.enabled() {
            events
                .emit(
                    "warning",
                    json!({
                        "severity": "warning",
                        "code": "link_skipped",
                        "stage": "inspect",
                        "path": path,
                        "recoverable": true,
                        "message": message,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!("warning: {message} Link: {path:?}");
        }
    }
    let mut source_entries = book_input.entries;

    if source_entries.is_empty()
        && book_input.skipped_non_images == 0
        && book_input.skipped_links.is_empty()
    {
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
    // A ComicInfo.xml that cannot be read is no reason to refuse the book: it is made without
    // that metadata, and the person is told, as upstream does.
    let comic_info = match comic_info_xml.as_deref() {
        Some(bytes) => match metadata::parse_comic_info_xml(&metadata::comic_info_text(bytes)) {
            Ok(info) => Some(info),
            Err(reason) => {
                warn(
                    events,
                    "comic_info_unreadable",
                    "metadata",
                    &input_path,
                    &format!(
                        "ComicInfo.xml could not be read and was ignored; the book is made without it ({reason})."
                    ),
                )?;
                None
            }
        },
        None => None,
    };
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
    let skipped_non_images = skipped_non_images + book_input.skipped_non_images;
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

    // Upstream's two warnings about what it was given. Neither stops the run.
    let device = (width, height);
    let (smaller, measured) =
        mangapress_core::archive::smaller_than_device(&source_entries, device);
    let mut input_warnings: Vec<(&str, String)> = Vec::new();
    if mangapress_core::archive::looks_already_converted(&source_entries) {
        input_warnings.push((
            "source_already_converted",
            "These pages look like KCC already converted them. Converting them again will lower their quality.".to_string(),
        ));
    }
    // Upstream leaves Kindle Scribe profiles out of this one: their screens
    // are larger than most scans.
    if smaller * 4 > measured
        && !cli.upscale
        && !cli.stretch
        && !cli.webtoon
        && !profile.code.starts_with("KS")
    {
        input_warnings.push((
            "images_smaller_than_device",
            format!(
                "{smaller} of {measured} pages are smaller than the device's {}x{} screen. Consider --upscale (or --stretch) to make them easier to read.",
                device.0, device.1
            ),
        ));
    }
    for (code, message) in input_warnings {
        if events.enabled() {
            events
                .emit(
                    "warning",
                    json!({
                        "severity": "warning",
                        "code": code,
                        "stage": "inspect",
                        "path": input_path.clone(),
                        "recoverable": true,
                        "message": message,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!("warning: {message}");
        }
    }

    // A cover of the user's own choosing — named with `--cover`, or else
    // found in a `Covers` folder beside the input — read now so that a bad
    // path fails before any page is processed.
    let cover_path = cli.cover.clone().or_else(|| {
        let found = cover_by_convention(&input)?;
        if !quiet {
            eprintln!("using {} as the cover", found.display());
        }
        Some(found)
    });
    let custom_cover: Option<Vec<u8>> = match &cover_path {
        Some(path) => match mangapress_core::input::read_file(path) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                *failure = RunFailure::new(
                    "cover_read_failed",
                    "inspect",
                    true,
                    "Couldn't read the cover image.",
                    format!("reading cover image {}: {error}", path.display()),
                )
                .with_path(absolute_display(path));
                bail!("reading cover image {}: {error}", path.display());
            }
        },
        None => None,
    };

    let mut source_chapters = group_into_chapters(source_entries);

    // Pages labelled as the two halves of a spread — in the file `--spreads`
    // names, or else the one upstream's "Label Spreads" leaves beside the
    // input — are joined before anything looks at them, the cover included.
    let spread_labels = cli
        .spreads
        .clone()
        .map(|path| (path, true))
        .or_else(|| spread_labels_beside(&input).map(|path| (path, false)));
    let mut joined_spreads = spreads::Joined::default();
    if let Some((path, named)) = spread_labels {
        match read_spread_labels(&path) {
            Ok(positions) => {
                let right_to_left = cli.manga_style && !cli.webtoon;
                joined_spreads = match spreads::join_labelled_spreads(
                    &mut source_chapters,
                    &positions,
                    right_to_left,
                ) {
                    Ok(joined) => joined,
                    Err(error) => {
                        *failure = RunFailure::new(
                            "spread_join_failed",
                            "inspect",
                            true,
                            "Couldn't join the pages labelled as a spread.",
                            format!("joining labelled spreads: {error}"),
                        )
                        .with_path(absolute_display(&path));
                        bail!("joining labelled spreads: {error}");
                    }
                };
                if !quiet && !joined_spreads.joined.is_empty() {
                    eprintln!(
                        "joined {} labelled spread(s) from {}",
                        joined_spreads.joined.len(),
                        path.display()
                    );
                }
                if !joined_spreads.skipped.is_empty() {
                    let reasons: Vec<String> = joined_spreads
                        .skipped
                        .iter()
                        .map(|(position, reason)| match reason {
                            spreads::Skipped::NoPageAfter => {
                                format!("{position} has no page after it")
                            }
                            spreads::Skipped::PartOfPreviousPair => {
                                format!("{position} is already the second half of a pair")
                            }
                        })
                        .collect();
                    warn(
                        events,
                        "spread_labels_skipped",
                        "inspect",
                        &absolute_display(&path),
                        &format!(
                            "Some labelled spreads could not be joined: position {}.",
                            reasons.join("; position ")
                        ),
                    )?;
                }
            }
            Err(reason) if named => {
                *failure = RunFailure::new(
                    "spread_labels_read_failed",
                    "inspect",
                    true,
                    "Couldn't read the spread labels.",
                    format!("reading spread labels {}: {reason}", path.display()),
                )
                .with_path(absolute_display(&path));
                bail!("reading spread labels {}: {reason}", path.display());
            }
            // A file that merely happens to sit beside the input and isn't
            // spread labels is not this book's problem.
            Err(reason) => warn(
                events,
                "spread_labels_ignored",
                "inspect",
                &absolute_display(&path),
                &format!(
                    "Ignored {}: it is not a list of spread labels ({reason}).",
                    path.display()
                ),
            )?,
        }
    }

    // The cover is made from the book's first image as it came, apart from
    // whatever page processing does to that image later.
    // Webtoon mode has no separate cover upstream (its first image is a
    // strip, not a cover); the first cut page stands in for one.
    let cover_source: Option<Vec<u8>> = custom_cover.clone().or_else(|| {
        source_chapters
            .iter()
            .find_map(|chapter| chapter.pages.first())
            .filter(|_| !cli.webtoon)
            .map(|page| page.bytes.clone())
    });
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
    // A Kobo profile's EPUB is a "kepub" by name, as upstream names it —
    // unless asked not to, or the resolution is custom (upstream no longer
    // sees a Kobo profile then).
    let kepub = cli.format == Format::Epub
        && profile.family() == Family::Kobo
        && !cli.nokepub
        && cli.customwidth.unwrap_or(0) == 0
        && cli.customheight.unwrap_or(0) == 0;
    // What the events call the format is the format itself. The file's
    // extension is a different thing for a kepub, and reporting it as the
    // format ("kepub.epub") is not a value the protocol has.
    let format = format_name(cli.format);
    let extension = if kepub { "kepub.epub" } else { format };
    *failure = RunFailure::new(
        "output_plan_failed",
        "plan",
        true,
        "Couldn't prepare the output destination. Check its path and permissions.",
        "planning the output destination",
    )
    .with_manga(title.clone())
    .with_path(absolute_display(cli.output.as_deref().unwrap_or(&input)));
    let output_plan = output::plan(&input, cli.output.as_deref(), extension, kepub)
        .context("planning the output destination")?;
    let output_path = output_plan.path;
    if output_plan.collision {
        let (code, message) = if output_plan.input_collision {
            ("output_collision", "The requested output would overwrite the input, so a safe alternate filename will be used.")
        } else {
            (
                "output_exists",
                "The requested output already exists, so a safe alternate filename will be used.",
            )
        };
        if events.enabled() {
            events
                .emit(
                    "warning",
                    json!({
                        "severity": "warning",
                        "code": code,
                        "stage": "plan",
                        "manga": title.clone(),
                        "path": absolute_display(&output_path),
                        "recoverable": true,
                        "message": message,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!("warning: {message} Writing to {}", output_path.display());
        }
    }
    let output_path_absolute = absolute_display(&output_path);
    let staged_output = if cli.dry_run {
        None
    } else {
        let directory = output::parent(&output_path)?;
        *failure = RunFailure::new(
            "output_directory_create_failed",
            "write",
            true,
            "Couldn't create the selected output folder.",
            format!("creating output directory {}", directory.display()),
        )
        .with_manga(title.clone())
        .with_path(absolute_display(directory));
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating output directory {}", directory.display()))?;
        *failure = RunFailure::new(
            "output_plan_failed",
            "plan",
            true,
            "Couldn't prepare the output destination. Check its path and permissions.",
            format!("staging output beside {}", output_path.display()),
        )
        .with_manga(title.clone())
        .with_path(output_path_absolute.clone());
        Some(output::StagedOutput::new(&output_path).context("staging the output file")?)
    };
    events
        .emit(
            "stage",
            json!({
                "stage": "plan",
                "state": "completed",
                "manga": title.clone(),
                "output_path": output_path_absolute.clone(),
                "format": format,
                "profile": profile.code,
                "device": profile.display_name,
                "width": width,
                "height": height,
                "gray_levels": effective_palette(profile, cli.customwidth, cli.customheight).levels(),
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
                        "format": format,
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
            write_human_report(&mut std::io::stdout().lock(), |stdout| {
                writeln!(
                    stdout,
                    "dry run -- no pages will be processed, nothing will be written"
                )?;
                writeln!(stdout, "title: {title}")?;
                writeln!(stdout, "author: {author}")?;
                writeln!(stdout, "format: {format}")?;
                writeln!(
                    stdout,
                    "device: {} ({width}x{height})",
                    profile.display_name
                )?;
                writeln!(stdout, "chapters: {total_chapters}, pages: {total_pages}")?;
                writeln!(stdout, "would write: {}", output_path.display())
            })
            .context("writing the dry-run summary")?;
        }
        return Ok(());
    }

    // Webtoon mode works on strips, not pages: each chapter's images are
    // joined and cut again before anything else happens to them, so the
    // page count from here on is the cut pages'.
    let (source_chapters, total_pages) = if cli.webtoon {
        let mut chapters = source_chapters;
        for chapter in &mut chapters {
            let sources: Vec<&[u8]> = chapter
                .pages
                .iter()
                .map(|page| page.bytes.as_slice())
                .collect();
            match mangapress_core::webtoon::pages_from_chapter(&sources, device) {
                Ok(pages) => {
                    chapter.pages = pages
                        .into_iter()
                        .map(|bytes| Page {
                            extension: "png".to_string(),
                            bytes,
                            ..Default::default()
                        })
                        .collect();
                }
                Err(error) => {
                    *failure = RunFailure::new(
                        "webtoon_split_failed",
                        "process",
                        true,
                        format!("Couldn't cut chapter '{}' into pages.", chapter.title),
                        format!("cutting chapter '{}' into pages: {error}", chapter.title),
                    )
                    .with_manga(title.clone())
                    .with_chapter(chapter.title.clone());
                    bail!("cutting chapter '{}' into pages: {error}", chapter.title);
                }
            }
        }
        let total = chapters.iter().map(|chapter| chapter.pages.len()).sum();
        (chapters, total)
    } else {
        (source_chapters, total_pages)
    };

    let pipeline_options = PipelineOptions {
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
        // Pages lying directly in the book have no folder to name them: the book's title does,
        // as it does in the contents.
        let chapter_title = if chapter.relative_path.as_os_str().is_empty() {
            title.clone()
        } else {
            chapter.title.clone()
        };
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
            chapter_index == 0,
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
            Ok(processed) => {
                for &position in &processed.truncated_sources {
                    let file = chapter.pages[position]
                        .source_path
                        .as_ref()
                        .map(|path| path.display().to_string());
                    let message = format!(
                        "Page {} of chapter '{}'{} ends before its image does: what was read is kept and the rest of the page is blank.",
                        position + 1,
                        chapter_title,
                        file.as_ref().map(|file| format!(" ({file})")).unwrap_or_default(),
                    );
                    if events.enabled() {
                        events
                            .emit(
                                "warning",
                                json!({
                                    "severity": "warning",
                                    "code": "page_truncated",
                                    "stage": "process",
                                    "path": file.clone().unwrap_or_else(|| input_path.clone()),
                                    "recoverable": true,
                                    "message": message,
                                    "chapter": chapter_title.clone(),
                                    "page": position + 1,
                                }),
                            )
                            .map_err(event_write_failure)?;
                    } else {
                        eprintln!("warning: {message}");
                    }
                }
                processed.pages
            }
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
                bail!("{}", failure.diagnostic);
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
                "format": format,
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
    // The cover, and whether upstream would also put it in a CBZ: only when
    // it is not simply the first page (the user's own, or smart-cropped).
    let cover: Option<(Vec<u8>, bool)> = match cover_source.as_deref() {
        Some(source) if cli.format != Format::Pdf => {
            match cover::build_cover_reporting(
                source,
                &cover::CoverOptions {
                    target: pipeline_options.target_resolution(),
                    right_to_left: cli.manga_style && !cli.webtoon,
                    smart_crop: cli.smartcovercrop,
                    fill: cli.coverfill,
                    force_color: cli.forcecolor,
                    jpeg_quality: cli.jpeg_quality.unwrap_or_else(|| {
                        effective_default_jpeg_quality(profile, cli.customwidth, cli.customheight)
                    }),
                },
            ) {
                Ok(cover) => Some(cover),
                Err(error) => {
                    *failure = RunFailure::new(
                        "cover_build_failed",
                        "package",
                        true,
                        "Couldn't make the cover from that image.",
                        format!("building the cover: {error}"),
                    )
                    .with_manga(title.clone());
                    bail!("building the cover: {error}");
                }
            }
        }
        _ => None,
    };

    let output_result = match cli.format {
        Format::Auto => unreachable!("--format auto was resolved above"),
        Format::Epub => epub::build_epub(
            &processed_chapters,
            &epub::EpubOptions {
                title: title.clone(),
                authors: resolved.authors,
                language: cli.language.clone(),
                reading_direction: ReadingDirection {
                    right_to_left: cli.manga_style && !cli.webtoon,
                },
                description: resolved.summary,
                nested_toc: cli.nested_toc,
                kindle: profile.family() == Family::Kindle,
                // Upstream's Kindle fixed-layout block is for a Kindle
                // profile at its format-specific target; overriding either
                // dimension makes it upstream's "Custom" profile, which
                // gets none.
                kindle_resolution: (profile.family() == Family::Kindle
                    && cli.customwidth.unwrap_or(0) == 0
                    && cli.customheight.unwrap_or(0) == 0)
                    .then_some((width, height)),
                invert_direction: cli.invertdirection,
                spread_shift: cli.spreadshift,
                one_page_landscape: cli.onepagelandscape,
                cover: cover.as_ref().map(|(bytes, _)| bytes.clone()),
                // A bookmark counts source pages; joining spreads moved
                // the ones after each pair up by one.
                bookmarks: comic_info
                    .as_ref()
                    .map(|info| {
                        info.bookmarks
                            .iter()
                            .map(|(page, title)| {
                                let page = joined_spreads.position_after(*page as usize);
                                (page as u32, title.clone())
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                series: resolved.series.map(|name| (name, resolved.series_position)),
                modified: utc_timestamp(std::time::SystemTime::now()),
            },
        )?,
        Format::Cbz => {
            let keep_xml = cli
                .keepcomicinfo
                .then_some(comic_info_xml.as_deref())
                .flatten();
            let cbz_cover = cover
                .as_ref()
                .filter(|(_, smart_cropped)| custom_cover.is_some() || *smart_cropped)
                .map(|(bytes, _)| bytes.as_slice());
            cbz_out::build_cbz(&processed_chapters, keep_xml, cbz_cover)?
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
                "format": format,
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
    staged_output
        .expect("non-dry-run output was staged before processing")
        .write(&output_bytes)
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
                "format": format,
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
#[path = "main_tests.rs"]
mod tests;
