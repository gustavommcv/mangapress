mod args;
mod assembly;
mod configuration;
mod discovery;
mod inspection;
mod output;
mod planning;
mod preparation;
mod processing;
mod protocol;
mod publication;
mod reporting;

use anyhow::Context;
use args::Cli;
#[cfg(test)]
use args::{automatic_format, Format};
use assembly::BookAssets;
use clap::{error::ErrorKind, Parser};
#[cfg(test)]
use discovery::COVERS_FOLDER;
#[cfg(test)]
use discovery::{cover_by_convention, read_spread_labels, spread_labels_beside};
use inspection::ReadInput;
#[cfg(test)]
use mangapress_core::ebook::Page;
#[cfg(test)]
use mangapress_core::pipeline::OutputFormat;
#[cfg(test)]
use mangapress_core::pipeline::{CroppingMode, PipelineOptions, SplitterMode};
#[cfg(test)]
use mangapress_core::profile::Profile;
use planning::{BookSummary, Outcome};
use preparation::PreparedPages;
#[cfg(test)]
use processing::process_chapter_pages;
use processing::SourceChapters;
use protocol::{event_write_failure, EventSink, RunFailure};
use publication::BookCounts;
#[cfg(test)]
use reporting::utc_timestamp;
use reporting::write_human_report;
use serde_json::json;
use std::ffi::OsString;
use std::io::Write as _;
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
        metadata,
    } = inspection::read(&conversion, quiet, events, failure)?;
    let title = metadata.resolved.title.as_str();
    let PreparedPages {
        source_chapters,
        total_chapters,
        total_pages,
        custom_cover,
        cover_source,
        joined_spreads,
    } = preparation::prepare(book_input, &conversion, title, quiet, events, failure)?;
    let Outcome::Convert(planned) = planning::prepare(
        &conversion,
        &BookSummary {
            title,
            author: &metadata.author,
            chapters: total_chapters,
            pages: total_pages,
        },
        events,
        failure,
    )?
    else {
        return Ok(());
    };
    let processed = processing::process_book(
        SourceChapters {
            chapters: source_chapters,
            total_chapters,
            total_pages,
        },
        &conversion,
        title,
        quiet,
        events,
        failure,
    )?;
    let assembled = assembly::assemble(
        metadata,
        &processed,
        BookAssets {
            cover_source: cover_source.as_deref(),
            custom_cover: custom_cover.is_some(),
            joined_spreads: &joined_spreads,
        },
        &conversion,
        &planned,
        events,
        failure,
    )?;
    publication::publish(
        assembled,
        planned,
        &conversion,
        BookCounts {
            chapters: total_chapters,
            source_pages: processed.total_pages,
            output_pages: processed.page_count,
        },
        quiet,
        events,
        failure,
    )
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
