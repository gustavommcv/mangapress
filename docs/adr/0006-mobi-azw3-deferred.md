# 6. Defer MOBI/AZW3 output

## Status

Accepted.

## Context

Upstream KCC generates MOBI/AZW3 by shelling out to Amazon's closed-source
`kindlegen` binary (no longer officially distributed — README tells users
to extract it from "Kindle Previewer"), then patches EXTH metadata records
directly in the resulting file's bytes (`dualmetafix.py`, itself GPLv3 —
see `0007-gplv3-boundary-kcc-image-rs.md`) because `kindlegen` doesn't
expose a way to set them correctly at generation time.

There is no mature Rust crate for authoring MOBI/AZW3, and reimplementing
the format from scratch is a substantial, mostly-unrelated undertaking.
The project's primary target reader is KOReader, which already has strong
native support for fixed-layout EPUB — arguably a better experience for
manga than MOBI in the first place. EPUB is also the project's explicitly
stated priority output format.

## Decision

v1 supports EPUB, CBZ, and PDF output only. MOBI/AZW3 is not implemented
and not silently stubbed as an error deep in the pipeline — the CLI's
`--format` flag simply doesn't offer it as an option yet (see
`mangapress-cli/src/args.rs`).

## Consequences

If MOBI/AZW3 support becomes necessary later, the lowest-effort path is
almost certainly reproducing KCC's own approach: shell out to `kindlegen`
if present on `PATH`, fail with a clear message if not — not
reimplementing the format natively.
