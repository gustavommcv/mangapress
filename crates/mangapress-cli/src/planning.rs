//! Plan and stage an output, or report a read-only dry run, before page processing.

use crate::args::{format_name, Format};
use crate::configuration::ResolvedConversion;
use crate::output::{self, StagedOutput};
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use crate::reporting::{absolute_display, write_human_report};
use anyhow::Context;
use mangapress_core::pipeline::effective_palette;
use mangapress_core::profile::Family;
use serde_json::json;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Source counts and resolved metadata shared by plan events and the dry-run report.
pub(super) struct BookSummary<'a> {
    pub(super) title: &'a str,
    pub(super) author: &'a str,
    pub(super) chapters: usize,
    pub(super) pages: usize,
}

/// The destination and existing staging value consumed by later publication.
pub(super) struct PlannedOutput {
    pub(super) output_path: PathBuf,
    pub(super) output_path_absolute: String,
    pub(super) format: &'static str,
    pub(super) extension: &'static str,
    pub(super) staged_output: Option<StagedOutput>,
}

/// A dry-run report is complete; only a conversion proceeds to page processing.
pub(super) enum Outcome {
    DryRun,
    Convert(PlannedOutput),
}

pub(super) fn prepare<W: std::io::Write + Send>(
    conversion: &ResolvedConversion,
    book: &BookSummary<'_>,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<Outcome> {
    let ResolvedConversion {
        cli,
        input,
        profile,
        width,
        height,
        ..
    } = conversion;
    let BookSummary {
        title,
        chapters: total_chapters,
        pages: total_pages,
        ..
    } = *book;
    events
        .emit(
            "stage",
            json!({
                "stage": "plan",
                "state": "started",
                "manga": title.to_owned(),
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
    .with_manga(title.to_owned())
    .with_path(absolute_display(cli.output.as_deref().unwrap_or(input)));
    let output_plan = output::plan(input, cli.output.as_deref(), extension, kepub)
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
                        "manga": title.to_owned(),
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
        Some(stage_destination(
            &output_path,
            &output_path_absolute,
            title,
            failure,
        )?)
    };
    events
        .emit(
            "stage",
            json!({
                "stage": "plan",
                "state": "completed",
                "manga": title.to_owned(),
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

    let planned = PlannedOutput {
        output_path,
        output_path_absolute,
        format,
        extension,
        staged_output,
    };
    if cli.dry_run {
        report_dry_run(conversion, book, &planned, events)?;
        Ok(Outcome::DryRun)
    } else {
        Ok(Outcome::Convert(planned))
    }
}

fn stage_destination(
    output_path: &Path,
    output_path_absolute: &str,
    title: &str,
    failure: &mut RunFailure,
) -> anyhow::Result<StagedOutput> {
    let directory = output::parent(output_path)?;
    *failure = RunFailure::new(
        "output_directory_create_failed",
        "write",
        true,
        "Couldn't create the selected output folder.",
        format!("creating output directory {}", directory.display()),
    )
    .with_manga(title.to_owned())
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
    .with_manga(title.to_owned())
    .with_path(output_path_absolute.to_owned());
    output::StagedOutput::new(output_path).context("staging the output file")
}

fn report_dry_run<W: std::io::Write + Send>(
    conversion: &ResolvedConversion,
    book: &BookSummary<'_>,
    planned: &PlannedOutput,
    events: &EventSink<W>,
) -> anyhow::Result<()> {
    let ResolvedConversion {
        profile,
        width,
        height,
        ..
    } = conversion;
    let BookSummary {
        title,
        author,
        chapters: total_chapters,
        pages: total_pages,
    } = *book;
    let PlannedOutput {
        output_path,
        output_path_absolute,
        format,
        ..
    } = planned;
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
    Ok(())
}

#[cfg(test)]
mod tests;
