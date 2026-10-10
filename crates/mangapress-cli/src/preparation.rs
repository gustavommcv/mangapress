//! Prepare source pages and covers before planning or processing a book.

use crate::configuration::ResolvedConversion;
use crate::discovery::{cover_by_convention, read_spread_labels, spread_labels_beside};
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use crate::reporting::{absolute_display, warn};
use anyhow::bail;
use mangapress_core::archive::{BookInput, SourceEntry};
use mangapress_core::ebook::{group_into_chapters, spreads, Chapter};
use serde_json::json;

/// Existing core pages and join mapping, with the cover inputs used later.
pub(super) struct PreparedPages {
    pub(super) source_chapters: Vec<Chapter>,
    pub(super) total_chapters: usize,
    pub(super) total_pages: usize,
    pub(super) custom_cover: Option<Vec<u8>>,
    pub(super) cover_source: Option<Vec<u8>>,
    pub(super) joined_spreads: spreads::Joined,
}

pub(super) fn prepare<W: std::io::Write + Send>(
    book_input: BookInput,
    conversion: &ResolvedConversion,
    title: &str,
    quiet: bool,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<PreparedPages> {
    let cli = &conversion.cli;
    let source_entries = filter_and_warn(book_input, conversion, events, failure)?;
    let custom_cover = read_custom_cover(conversion, quiet, failure)?;
    let mut source_chapters = group_into_chapters(source_entries);
    let joined_spreads = join_spreads(&mut source_chapters, conversion, quiet, events, failure)?;
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
                "manga": title,
                "chapters": total_chapters,
                "pages": total_pages,
            }),
        )
        .map_err(event_write_failure)?;

    Ok(PreparedPages {
        source_chapters,
        total_chapters,
        total_pages,
        custom_cover,
        cover_source,
        joined_spreads,
    })
}

fn filter_and_warn<W: std::io::Write + Send>(
    book_input: BookInput,
    conversion: &ResolvedConversion,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<Vec<SourceEntry>> {
    let ResolvedConversion {
        cli,
        input,
        input_path,
        profile,
        width,
        height,
        ..
    } = conversion;
    let source_entries = book_input.entries;

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
    let device = (*width, *height);
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

    Ok(source_entries)
}

fn read_custom_cover(
    conversion: &ResolvedConversion,
    quiet: bool,
    failure: &mut RunFailure,
) -> anyhow::Result<Option<Vec<u8>>> {
    let ResolvedConversion { cli, input, .. } = conversion;
    // A cover of the user's own choosing — named with `--cover`, or else
    // found in a `Covers` folder beside the input — read now so that a bad
    // path fails before any page is processed.
    let cover_path = cli.cover.clone().or_else(|| {
        let found = cover_by_convention(input)?;
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

    Ok(custom_cover)
}

fn join_spreads<W: std::io::Write + Send>(
    source_chapters: &mut Vec<Chapter>,
    conversion: &ResolvedConversion,
    quiet: bool,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<spreads::Joined> {
    let ResolvedConversion { cli, input, .. } = conversion;
    // Pages labelled as the two halves of a spread — in the file `--spreads`
    // names, or else the one upstream's "Label Spreads" leaves beside the
    // input — are joined before anything looks at them, the cover included.
    let spread_labels = cli
        .spreads
        .clone()
        .map(|path| (path, true))
        .or_else(|| spread_labels_beside(input).map(|path| (path, false)));
    let mut joined_spreads = spreads::Joined::default();
    if let Some((path, named)) = spread_labels {
        match read_spread_labels(&path) {
            Ok(positions) => {
                let right_to_left = cli.manga_style && !cli.webtoon;
                joined_spreads = match spreads::join_labelled_spreads(
                    source_chapters,
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

    Ok(joined_spreads)
}

#[cfg(test)]
mod tests;
