use crate::configuration::{self, ResolvedConversion};
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use anyhow::bail;
use mangapress_core::ebook::Chapter;
use mangapress_core::ebook::Page;
use mangapress_core::pipeline::{process_page, PipelineOptions, ProcessedPage};
use rayon::prelude::*;
use serde_json::json;
use std::io::{IsTerminal, Write as _};
use std::sync::Mutex;

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
pub(super) struct PageProcessingFailure {
    pub(super) page: usize,
    pub(super) diagnostic: String,
}

/// A chapter's source pages, processed.
#[derive(Debug)]
pub(super) struct ProcessedChapter {
    pub(super) pages: Vec<Page>,
    /// The positions (zero-based) of the source pages whose file ended before the image did.
    pub(super) truncated_sources: Vec<usize>,
}

pub(super) fn process_chapter_pages(
    pages: &[Page],
    options: &PipelineOptions,
    first_chapter: bool,
    on_page_done: impl Fn(usize, usize) -> std::io::Result<()> + Sync,
) -> Result<ProcessedChapter, PageProcessingFailure> {
    let progress = Mutex::new((vec![false; pages.len()], 0usize));
    let outputs: Vec<Vec<ProcessedPage>> = pages
        .par_iter()
        .enumerate()
        .map(|(page_index, source_page)| {
            let page_number = page_index + 1;
            // The book's first page is the one upstream leaves uncropped
            // when it is a color page (a cover).
            let is_first_page = first_chapter && page_index == 0;
            let result =
                process_page(&source_page.bytes, options, is_first_page).map_err(|error| {
                    PageProcessingFailure {
                        page: page_number,
                        diagnostic: match &source_page.source_path {
                            Some(path) => format!("image {path:?}: {error}"),
                            None => error.to_string(),
                        },
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
    let mut truncated_sources = Vec::new();
    for (position, (source, page_outputs)) in pages.iter().zip(outputs).enumerate() {
        if page_outputs.iter().any(|page| page.source_truncated) {
            truncated_sources.push(position);
        }
        for (piece, page) in page_outputs.into_iter().enumerate() {
            flattened.push(Page {
                source_path: source.source_path.clone(),
                extension: page.extension,
                bytes: page.bytes,
                black_background: page.black_background,
                role: page.role,
                continues_source_page: piece > 0,
            });
        }
    }
    Ok(ProcessedChapter {
        pages: flattened,
        truncated_sources,
    })
}

/// Existing prepared chapters and source counts entering page execution.
pub(super) struct SourceChapters {
    pub(super) chapters: Vec<Chapter>,
    pub(super) total_chapters: usize,
    pub(super) total_pages: usize,
}

/// Page results and the same options/counts consumed by later book assembly.
pub(super) struct ProcessedBook {
    pub(super) chapters: Vec<Chapter>,
    pub(super) total_pages: usize,
    pub(super) page_count: usize,
    pub(super) pipeline_options: PipelineOptions,
}

struct ChapterContext<'a, W: std::io::Write + Send> {
    title: &'a str,
    input_path: &'a str,
    total_chapters: usize,
    total_pages: usize,
    pipeline_options: &'a PipelineOptions,
    quiet: bool,
    progress_is_tty: bool,
    events: &'a EventSink<W>,
}

pub(super) fn process_book<W: std::io::Write + Send>(
    source: SourceChapters,
    conversion: &ResolvedConversion,
    title: &str,
    quiet: bool,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<ProcessedBook> {
    let SourceChapters {
        chapters: source_chapters,
        total_chapters,
        total_pages,
    } = source;
    let ResolvedConversion {
        cli,
        profile,
        output_format,
        ..
    } = conversion;
    let (source_chapters, total_pages) =
        cut_webtoon(source_chapters, total_pages, conversion, title, failure)?;
    let pipeline_options = configuration::pipeline_options(cli, profile, *output_format);

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
                "manga": title.to_owned(),
                "chapters": total_chapters,
                "pages": total_pages,
            }),
        )
        .map_err(event_write_failure)?;

    let context = ChapterContext {
        title,
        input_path: &conversion.input_path,
        total_chapters,
        total_pages,
        pipeline_options: &pipeline_options,
        quiet,
        progress_is_tty,
        events,
    };
    let (processed_chapters, page_count) = process_chapters(source_chapters, &context, failure)?;
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
                "manga": title.to_owned(),
                "source_pages": total_pages,
                "output_pages": page_count,
            }),
        )
        .map_err(event_write_failure)?;

    Ok(ProcessedBook {
        chapters: processed_chapters,
        total_pages,
        page_count,
        pipeline_options,
    })
}

fn cut_webtoon(
    source_chapters: Vec<Chapter>,
    total_pages: usize,
    conversion: &ResolvedConversion,
    title: &str,
    failure: &mut RunFailure,
) -> anyhow::Result<(Vec<Chapter>, usize)> {
    let cli = &conversion.cli;
    let device = (conversion.width, conversion.height);
    // Webtoon mode works on strips, not pages: each chapter's images are
    // joined and cut again before anything else happens to them, so the
    // page count from here on is the cut pages'.
    Ok(if cli.webtoon {
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
                    .with_manga(title.to_owned())
                    .with_chapter(chapter.title.to_owned());
                    bail!("cutting chapter '{}' into pages: {error}", chapter.title);
                }
            }
        }
        let total = chapters.iter().map(|chapter| chapter.pages.len()).sum();
        (chapters, total)
    } else {
        (source_chapters, total_pages)
    })
}

fn process_chapters<W: std::io::Write + Send>(
    source_chapters: Vec<Chapter>,
    context: &ChapterContext<'_, W>,
    failure: &mut RunFailure,
) -> anyhow::Result<(Vec<Chapter>, usize)> {
    let ChapterContext {
        title,
        input_path,
        total_chapters,
        total_pages,
        pipeline_options,
        quiet,
        progress_is_tty,
        events,
    } = *context;
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
            title.to_owned()
        } else {
            chapter.title.to_owned()
        };
        let chapter_source_len = chapter.pages.len();
        events
            .emit(
                "chapter",
                json!({
                    "state": "started",
                    "stage": "process",
                    "manga": title.to_owned(),
                    "chapter": chapter_title.to_owned(),
                    "chapter_index": chapter_index + 1,
                    "chapter_count": total_chapters,
                    "source_pages": chapter_source_len,
                }),
            )
            .map_err(event_write_failure)?;
        let pages = match process_chapter_pages(
            &chapter.pages,
            pipeline_options,
            chapter_index == 0,
            |done_in_chapter, page_number| {
                let done = source_pages_done + done_in_chapter;
                if events.enabled() {
                    events.emit(
                        "page",
                        json!({
                            "state": "completed",
                            "stage": "process",
                            "manga": title.to_owned(),
                            "chapter": chapter_title.to_owned(),
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
                warn_truncated_pages(
                    &chapter,
                    &chapter_title,
                    &processed.truncated_sources,
                    context,
                )?;
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
                .with_manga(title.to_owned())
                .with_chapter(chapter_title.to_owned())
                .with_page(error.page)
                .with_path(input_path.to_owned());
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
                    "manga": title.to_owned(),
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
    Ok((processed_chapters, page_count))
}

fn warn_truncated_pages<W: std::io::Write + Send>(
    chapter: &Chapter,
    chapter_title: &str,
    truncated_sources: &[usize],
    context: &ChapterContext<'_, W>,
) -> anyhow::Result<()> {
    let input_path = context.input_path;
    let events = context.events;
    for &position in truncated_sources {
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
                        "path": file.clone().unwrap_or_else(|| input_path.to_owned()),
                        "recoverable": true,
                        "message": message,
                        "chapter": chapter_title.to_owned(),
                        "page": position + 1,
                    }),
                )
                .map_err(event_write_failure)?;
        } else {
            eprintln!("warning: {message}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
