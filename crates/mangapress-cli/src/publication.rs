//! Coordinate publication and its final report using the existing staged output.

use crate::assembly::AssembledBook;
use crate::configuration::ResolvedConversion;
use crate::planning::PlannedOutput;
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use anyhow::Context;
use serde_json::json;

/// Preserve the distinct chapter, effective source-page and output-page counts.
pub(super) struct BookCounts {
    pub(super) chapters: usize,
    pub(super) source_pages: usize,
    pub(super) output_pages: usize,
}

pub(super) fn publish<W: std::io::Write + Send>(
    book: AssembledBook,
    planned: PlannedOutput,
    conversion: &ResolvedConversion,
    counts: BookCounts,
    quiet: bool,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<()> {
    let AssembledBook {
        bytes: output_bytes,
        title,
        author: result_author,
    } = book;
    let BookCounts {
        chapters: total_chapters,
        source_pages,
        output_pages,
    } = counts;
    let PlannedOutput {
        output_path,
        output_path_absolute,
        format,
        staged_output,
        extension: _,
    } = planned;
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
                "profile": conversion.profile.code,
                "width": conversion.width,
                "height": conversion.height,
                "chapters": total_chapters,
                "source_pages": source_pages,
                "output_pages": output_pages,
                "output_path": output_path_absolute,
                "bytes": output_bytes.len(),
                "written": true,
            }),
        )
        .map_err(event_write_failure)?;

    Ok(())
}

#[cfg(test)]
mod tests;
