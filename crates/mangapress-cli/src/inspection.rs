//! Read source entries and metadata before filtering, planning or processing pages.

use crate::args::MetadataTitle;
use crate::configuration::ResolvedConversion;
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use crate::reporting::{absolute_display, warn};
use anyhow::{bail, Context};
use mangapress_core::archive::{read_book, BookInput};
use mangapress_core::metadata::{self, MetadataTitleMode};
use mangapress_core::pipeline::effective_palette;
use serde_json::json;

/// Raw entries and diagnostics retained for the later page-inspection steps.
pub(super) struct ReadInput {
    pub(super) book_input: BookInput,
    pub(super) metadata: InputMetadata,
}

/// Parsed, resolved and raw metadata each have a distinct later use.
pub(super) struct InputMetadata {
    pub(super) resolved: metadata::ResolvedMetadata,
    pub(super) author: String,
    pub(super) comic_info: Option<metadata::ComicInfo>,
    pub(super) comic_info_xml: Option<Vec<u8>>,
}

pub(super) fn read<W: std::io::Write + Send>(
    conversion: &ResolvedConversion,
    quiet: bool,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<ReadInput> {
    let ResolvedConversion {
        cli,
        input,
        input_path,
        profile,
        width,
        height,
        ..
    } = conversion;
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
    let mut book_input =
        read_book(input).with_context(|| format!("reading input from {}", input.display()))?;
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

    if book_input.entries.is_empty()
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

    let metadata = read_metadata(&mut book_input, conversion, &fallback_title, quiet, events)?;
    Ok(ReadInput {
        book_input,
        metadata,
    })
}

fn read_metadata<W: std::io::Write + Send>(
    book_input: &mut BookInput,
    conversion: &ResolvedConversion,
    fallback_title: &str,
    quiet: bool,
    events: &EventSink<W>,
) -> anyhow::Result<InputMetadata> {
    let ResolvedConversion {
        cli, input_path, ..
    } = conversion;
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
    let comic_info_xml = metadata::extract_comic_info_entry(&mut book_input.entries);
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
                    input_path,
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
        fallback_title,
        match cli.metadatatitle {
            MetadataTitle::SeriesOnly => MetadataTitleMode::SeriesOnly,
            MetadataTitle::Combine => MetadataTitleMode::Combine,
            MetadataTitle::TitleOnly => MetadataTitleMode::TitleOnly,
        },
    );
    let author = resolved.authors.join(", ");
    events
        .emit(
            "stage",
            json!({
                "stage": "metadata",
                "state": "completed",
                "manga": resolved.title.clone(),
                "title": resolved.title.clone(),
                "author": author.clone(),
                "comic_info_found": comic_info.is_some(),
            }),
        )
        .map_err(event_write_failure)?;

    Ok(InputMetadata {
        resolved,
        author,
        comic_info,
        comic_info_xml,
    })
}

#[cfg(test)]
mod tests;
