//! Assemble converted pages with the existing core cover and book builders.

use crate::args::Format;
use crate::configuration::ResolvedConversion;
use crate::inspection::InputMetadata;
use crate::planning::PlannedOutput;
use crate::processing::ProcessedBook;
use crate::protocol::{event_write_failure, EventSink, RunFailure};
use crate::reporting::utc_timestamp;
use anyhow::bail;
use mangapress_core::ebook::{cbz_out, cover, epub, pdf, spreads};
use mangapress_core::manga::ReadingDirection;
use mangapress_core::pipeline::{effective_default_jpeg_quality, PipelineOptions};
use mangapress_core::profile::Family;
use serde_json::json;

/// Cover inputs and source-page join mapping retained from preparation.
pub(super) struct BookAssets<'a> {
    pub(super) cover_source: Option<&'a [u8]>,
    pub(super) custom_cover: bool,
    pub(super) joined_spreads: &'a spreads::Joined,
}

/// Completed bytes and existing metadata consumed by publication/reporting.
pub(super) struct AssembledBook {
    pub(super) bytes: Vec<u8>,
    pub(super) title: String,
    pub(super) author: String,
}

pub(super) fn assemble<W: std::io::Write + Send>(
    metadata: InputMetadata,
    processed: &ProcessedBook,
    assets: BookAssets<'_>,
    conversion: &ResolvedConversion,
    planned: &PlannedOutput,
    events: &EventSink<W>,
    failure: &mut RunFailure,
) -> anyhow::Result<AssembledBook> {
    let title = metadata.resolved.title.as_str();
    let format = planned.format;
    let extension = planned.extension;
    let output_path_absolute = &planned.output_path_absolute;
    events
        .emit(
            "stage",
            json!({
                "stage": "package",
                "state": "started",
                "manga": title.to_owned(),
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
    .with_manga(title.to_owned())
    .with_path(output_path_absolute.clone());
    let result_author = metadata.author.clone();
    let cover = build_cover(
        &assets,
        conversion,
        &processed.pipeline_options,
        title,
        failure,
    )?;
    let (output_bytes, title) = package_bytes(metadata, processed, &assets, conversion, &cover)?;
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

    Ok(AssembledBook {
        bytes: output_bytes,
        title,
        author: result_author,
    })
}

fn build_cover(
    assets: &BookAssets<'_>,
    conversion: &ResolvedConversion,
    pipeline_options: &PipelineOptions,
    title: &str,
    failure: &mut RunFailure,
) -> anyhow::Result<Option<(Vec<u8>, bool)>> {
    let cli = &conversion.cli;
    let profile = conversion.profile;
    let cover_source = assets.cover_source;
    // The cover, and whether upstream would also put it in a CBZ: only when
    // it is not simply the first page (the user's own, or smart-cropped).
    let cover: Option<(Vec<u8>, bool)> = match cover_source {
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
                    .with_manga(title.to_owned());
                    bail!("building the cover: {error}");
                }
            }
        }
        _ => None,
    };

    Ok(cover)
}

fn package_bytes(
    metadata: InputMetadata,
    processed: &ProcessedBook,
    assets: &BookAssets<'_>,
    conversion: &ResolvedConversion,
    cover: &Option<(Vec<u8>, bool)>,
) -> anyhow::Result<(Vec<u8>, String)> {
    let InputMetadata {
        resolved,
        author,
        comic_info,
        comic_info_xml,
    } = metadata;
    let title = resolved.title;
    let cli = &conversion.cli;
    let profile = conversion.profile;
    let (width, height) = (conversion.width, conversion.height);
    let processed_chapters = processed.chapters.as_slice();
    let custom_cover = assets.custom_cover;
    let joined_spreads = assets.joined_spreads;
    let output_result = match cli.format {
        Format::Auto => unreachable!("--format auto was resolved above"),
        Format::Epub => epub::build_epub(
            processed_chapters,
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
                .filter(|(_, smart_cropped)| custom_cover || *smart_cropped)
                .map(|(bytes, _)| bytes.as_slice());
            cbz_out::build_cbz(processed_chapters, keep_xml, cbz_cover)?
        }
        Format::Pdf => pdf::build_pdf(
            processed_chapters,
            &pdf::PdfOptions {
                title: title.clone(),
                author: author.clone(),
            },
        )?,
    };
    Ok((output_result, title))
}

#[cfg(test)]
mod tests;
