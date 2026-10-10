use mangapress_core::ebook::Page;
use mangapress_core::pipeline::{process_page, PipelineOptions, ProcessedPage};
use rayon::prelude::*;
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
