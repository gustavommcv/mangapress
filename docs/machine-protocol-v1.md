# mangapress machine protocol version 1

This document is the normative contract for mangapress's machine-readable interface.

## Framing and handshake

Machine output is UTF-8 JSON Lines: every non-empty stdout line is one complete JSON object. Every
event has:

- `protocol_version`: `1`
- `tool`: `"mangapress"`
- `tool_version`: the executable's release version
- `sequence`: a contiguous one-based integer within the process
- `type`: the event type

```text
mangapress --protocol-version
```

returns one `protocol` event whose `capabilities` are `events`, `profiles`, and `nested_toc` (see
[ADR 0012](adr/0012-nested-toc-for-combined-volumes.md)). Mangabound must
require an exact supported protocol version and separately verify the release version and checksum.

## Conversion stream

```text
mangapress <input> --profile KV --output <book.epub> --json-events
mangapress <input> --profile KV --output <book.epub> --dry-run --json-events
```

The stream contains these event types:

| Type | Purpose |
|---|---|
| `stage` | `started`/`completed` transition for `inspect`, `metadata`, `plan`, `process`, `package`, or `write` |
| `chapter` | `started`/`completed` transition with chapter title/index and source/output page counts. Pages lying directly in the book (not in a folder) are one chapter, titled with the book's title |
| `page` | Completion of one source page with chapter, one-based page, global completed count, and total |
| `warning` | Recoverable issue with stable code, stage, message, and context |
| `error` | Fatal issue with stable code, stage, context, message, and diagnostic detail |
| `result` | Successful plan, conversion, or profile-list result; always the last event on success |
| `profile` | One device profile from structured `--list-profiles` output |
| `protocol` | Compatibility handshake |

Dry-run emits inspect, metadata, and plan stages followed by a result with `dry_run: true` and
`written: false`; it emits no process/page/package/write events and creates nothing. A successful
conversion result includes title, author, format, profile, resolution, chapter count, source and
output page counts, absolute output path, byte count, and `written: true`.

`format` is always one of `epub`, `cbz` and `pdf`, in every event that carries it. It is the
format, not the file's extension: a Kobo profile's EPUB is named `.kepub.epub` and its format is
still `epub`.

Output names follow the source filename or whole folder name, not the resolved book title.
An existing destination, including an explicit file, selects a numbered safe alternate; plans
and results report the path actually chosen. Keep using `output_path`, not a reconstructed
name. See [ADR 0016](adr/0016-safe-output-planning-and-publication.md).

A dry-run checks names and existing ancestors without creating a directory, probe, or file.
It does not guarantee future ACL access or free space. A real run creates missing parents
and stages a private sibling file before processing pages. Publication never overwrites;
a destination created after planning fails as `output_write_failed`, with no completed result.

Page work may finish on different worker threads, but events are serialized. `sequence` and the
page event's global `completed` value always increase by exactly one. A page event's `page` is its
one-based position within the named chapter.

## Profiles

`mangapress --list-profiles --json-events` emits `profile` events in the same declaration order as
human `--list-profiles`, followed by a result. A profile contains code, display name, width, height,
gray levels, and family. `OTHER` legitimately reports a zero built-in resolution and requires CLI
width/height overrides when used for conversion.

Profile events report built-in dimensions, not a format-specific target. Plan-stage and
conversion-result `width`/`height` fields report the effective processing resolution. For
`KDX` CBZ output, that is 824 × 1200 unless either custom dimension is set; its profile event
still reports 824 × 1000. EPUB/PDF retain the built-in dimensions, as in KCC 12.0.0.
An unmodified Scribe EPUB caps its target width at 1920; CBZ/PDF and custom dimensions
do not. Its EPUB `original-resolution` metadata uses the effective target as well.

## Issues

Book inspection filters non-image payloads before reading them, while retaining root-level
`ComicInfo.xml`. Ignored files still count toward `skipped_non_images`, including on a dry run.
For folder input, descendant links are followed only to regular files inside the real input
root. External, unresolvable, and directory links are excluded and reported separately as
`link_skipped` warnings at `inspect`, on dry-run and conversion, in natural path order. Each
warning's `path` names the link, not its target; `message` gives the reason without target
details. If no pages remain, these warnings precede `no_page_images`. See
[ADR 0017](adr/0017-folder-links-stay-inside-the-input.md) for scope and limitations.

The [input limits](adr/0015-bounded-input-reads.md) use the existing failure codes: file/entry
limits are `input_read_failed`; image limits use the relevant processing, cover, or spread
failure. The error's `diagnostic` describes the exceeded limit; there is no completed result
or output write after that failure. A dry run checks entry bytes, not every image's decodability.

Every warning or error includes `severity`, stable `code`, `stage`, `recoverable`, and a user-facing
`message`. Optional context includes `manga`, `volume`, `chapter`, `page`, and `path`. Error events
also contain `diagnostic` for an explicitly expanded technical-details view. Consumers branch on
`code`, never on English text.

Version 1 issue codes are:

| Code | Stage | Meaning |
|---|---|---|
| `invalid_arguments` | `configuration` | clap rejected the invocation |
| `unknown_profile` | `configuration` | The requested profile does not exist |
| `nested_toc_unsupported_format` | `configuration` | Nested volume/chapter navigation was requested with a format other than EPUB |
| `invalid_resolution` | `configuration` | The effective target width or height is zero |
| `input_not_found` | `inspect` | The input path does not exist |
| `input_read_failed` | `inspect` | The folder or CBZ could not be read |
| `input_empty` | `inspect` | The input contains no files |
| `skipped_non_images` | `inspect` | Non-image entries were ignored |
| `link_skipped` | `inspect` | A folder link was excluded because it leads outside the input, is not a regular file, or could not be resolved |
| `no_page_images` | `inspect` | No recognized image entries remain |
| `source_already_converted` | `inspect` | The pages carry KCC's own file names; converting again loses quality |
| `images_smaller_than_device` | `inspect` | Over a quarter of the pages are smaller than the screen and nothing enlarges them |
| `cover_read_failed` | `inspect` | The cover image — `--cover`, or the one found in a `Covers` folder beside the input — could not be read |
| `spread_labels_read_failed` | `inspect` | The `--spreads` file could not be read, or is not a list of spread labels |
| `spread_labels_ignored` | `inspect` | A `.json` file beside the input is not a list of spread labels; the book is converted without it |
| `spread_labels_skipped` | `inspect` | Some labelled positions have no page to be joined with; the others were joined |
| `spread_join_failed` | `inspect` | Two pages labelled as a spread could not be joined into one image |
| `comic_info_unreadable` | `metadata` | ComicInfo.xml is not well-formed XML or cannot be decoded; it is ignored and the book is made without it |
| `metadata_parse_failed` | `metadata` | No longer emitted: an unreadable ComicInfo.xml is the warning `comic_info_unreadable` |
| `output_collision` | `plan` | The chosen output would overwrite the input; a safe name is used |
| `output_exists` | `plan` | Another entry occupies the chosen output; a safe name is used |
| `output_plan_failed` | `plan` | The destination has an invalid name/parent or could not be staged before processing |
| `output_directory_create_failed` | `write` | The output directory could not be created |
| `page_processing_failed` | `process` | A named page in a named chapter failed conversion |
| `webtoon_split_failed` | `process` | A chapter's strips could not be cut into pages |
| `cover_build_failed` | `package` | The cover could not be made from its image |
| `book_build_failed` | `package` | EPUB, CBZ, or PDF assembly failed |
| `output_write_failed` | `write` | The completed bytes could not be saved |
| `event_write_failed` | `protocol` | The JSON Lines stream itself could not be written |

Page-processing diagnostics include the one-based page position and, when available, the
original image's relative path. Joined spreads and generated webtoon pages do not have a
single original filename. The error's `path` continues to identify the input book, not the
image mentioned in `diagnostic`.

BMP input is supported for normal conversion, but cannot be embedded unchanged in EPUB.
EPUB with `--noprocessing` therefore rejects a BMP page during processing with
`page_processing_failed`; its diagnostic recommends removing the flag or choosing CBZ.
The decision uses the detected image format, even if the file has another recognized extension.
Dry-run still inspects entries without validating every image's decodability or embedding format.

If stdout closes or a JSON event cannot be written, the process fails with exit code 1; a
complete final error event may itself be impossible to emit. This differs from human profile
lists and dry-run summaries, which stop quietly when a pipe's reader closes early.

## Evolution rule

Consumers must ignore unknown object fields, event types, stages, and issue codes. Adding any of
those remains compatible with version 1. Removing a field, changing its JSON type or documented
meaning, changing one-object-per-line framing, or weakening sequence ordering requires a new
protocol version and ADR.
