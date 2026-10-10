mod args;
mod configuration;
mod discovery;
mod inspection;
mod output;
mod planning;
mod preparation;
mod processing;
mod protocol;
mod reporting;

use anyhow::{bail, Context};
#[cfg(test)]
use args::automatic_format;
use args::{Cli, Format};
use clap::{error::ErrorKind, Parser};
use configuration::ResolvedConversion;
#[cfg(test)]
use discovery::COVERS_FOLDER;
#[cfg(test)]
use discovery::{cover_by_convention, read_spread_labels, spread_labels_beside};
use inspection::{InputMetadata, ReadInput};
use mangapress_core::ebook::{cbz_out, cover, epub, pdf, Chapter, Page};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::pipeline::effective_default_jpeg_quality;
#[cfg(test)]
use mangapress_core::pipeline::OutputFormat;
#[cfg(test)]
use mangapress_core::pipeline::{CroppingMode, PipelineOptions, SplitterMode};
use mangapress_core::profile::Family;
#[cfg(test)]
use mangapress_core::profile::Profile;
use planning::{BookSummary, Outcome, PlannedOutput};
use preparation::PreparedPages;
use processing::process_chapter_pages;
use protocol::{event_write_failure, EventSink, RunFailure};
use reporting::{utc_timestamp, write_human_report};
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
    let conversion = configuration::resolve(cli, failure)?;
    let ReadInput {
        book_input,
        metadata:
            InputMetadata {
                resolved,
                author,
                comic_info,
                comic_info_xml,
            },
    } = inspection::read(&conversion, quiet, events, failure)?;
    let title = resolved.title;
    let PreparedPages {
        source_chapters,
        total_chapters,
        total_pages,
        custom_cover,
        cover_source,
        joined_spreads,
    } = preparation::prepare(book_input, &conversion, &title, quiet, events, failure)?;
    let Outcome::Convert(PlannedOutput {
        output_path,
        output_path_absolute,
        format,
        extension,
        staged_output,
    }) = planning::prepare(
        &conversion,
        &BookSummary {
            title: &title,
            author: &author,
            chapters: total_chapters,
            pages: total_pages,
        },
        events,
        failure,
    )?
    else {
        return Ok(());
    };
    let ResolvedConversion {
        cli,
        input: _,
        input_path,
        profile,
        width,
        height,
        output_format,
    } = conversion;
    let device = (width, height);

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

    let pipeline_options = configuration::pipeline_options(&cli, profile, output_format);

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
