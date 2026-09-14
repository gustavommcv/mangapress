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

returns one `protocol` event whose `capabilities` are `events` and `profiles`. Mangabound must
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
| `chapter` | `started`/`completed` transition with chapter title/index and source/output page counts |
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

Page work may finish on different worker threads, but events are serialized. `sequence` and the
page event's global `completed` value always increase by exactly one. A page event's `page` is its
one-based position within the named chapter.

## Profiles

`mangapress --list-profiles --json-events` emits `profile` events in the same declaration order as
human `--list-profiles`, followed by a result. A profile contains code, display name, width, height,
gray levels, and family. `OTHER` legitimately reports a zero built-in resolution and requires CLI
width/height overrides when used for conversion.

## Issues

Every warning or error includes `severity`, stable `code`, `stage`, `recoverable`, and a user-facing
`message`. Optional context includes `manga`, `volume`, `chapter`, `page`, and `path`. Error events
also contain `diagnostic` for an explicitly expanded technical-details view. Consumers branch on
`code`, never on English text.

Version 1 issue codes are:

| Code | Stage | Meaning |
|---|---|---|
| `invalid_arguments` | `configuration` | clap rejected the invocation |
| `unknown_profile` | `configuration` | The requested profile does not exist |
| `invalid_resolution` | `configuration` | The effective target width or height is zero |
| `input_not_found` | `inspect` | The input path does not exist |
| `input_read_failed` | `inspect` | The folder or CBZ could not be read |
| `input_empty` | `inspect` | The input contains no files |
| `skipped_non_images` | `inspect` | Non-image entries were ignored |
| `no_page_images` | `inspect` | No recognized image entries remain |
| `metadata_parse_failed` | `metadata` | ComicInfo.xml is invalid or unreadable |
| `output_collision` | `plan` | The chosen output would overwrite the input; a safe name is used |
| `output_directory_create_failed` | `write` | The output directory could not be created |
| `page_processing_failed` | `process` | A named page in a named chapter failed conversion |
| `book_build_failed` | `package` | EPUB, CBZ, or PDF assembly failed |
| `output_write_failed` | `write` | The completed bytes could not be saved |
| `event_write_failed` | `protocol` | The JSON Lines stream itself could not be written |

## Evolution rule

Consumers must ignore unknown object fields, event types, stages, and issue codes. Adding any of
those remains compatible with version 1. Removing a field, changing its JSON type or documented
meaning, changing one-object-per-line framing, or weakening sequence ordering requires a new
protocol version and ADR.
