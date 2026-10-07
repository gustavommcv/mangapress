# 16. Follow KCC's non-overwriting output names and stage completed writes

Date: 2026-10-07

## Status

Accepted. Updates the output-write premise of ADR 0009; its original observations remain.

## Context

The October audit found that folder names with dots lost part of their name, `--output`
directories used book metadata instead of the source name, and existing outputs were
silently replaced. An interrupted final write could truncate a previous book. Invalid
destinations were often discovered only after processing every page.

The maintainer chose KCC's behavior where it is sound, with clig.dev guiding safety and
usability. At KCC 12.0.0, `getOutputFilename` uses the source stem (or the whole folder name)
and selects a numbered alternate whenever a destination exists, including an explicit file.
clig.dev does not prescribe a specific overwrite policy, but recommends avoiding accidental
destruction, checking input early, and keeping commands usable without interactive prompts.

## Decision

- Derive filenames from the input, not `--title` or `ComicInfo.xml`. A folder keeps its whole
  name, including dots; a file contributes its stem. Resolve `.` and `..` before naming a
  folder. Book titles, authors, metadata, and chapter organization are unchanged.
- Preserve every existing destination, including an explicit `--output file`. Keep this
  project's existing suffixes: ` (mangapress)`, ` (mangapress 2)`, and so on. Preserve the
  complete `.kepub.epub` extension when adding a suffix. Keep the existing Kobo stem rule
  for a default filename derived from a file.
- Count any existing entry as occupied, including directories and dangling symbolic links.
  Warn about the alternate path and report the actual path in the plan and result. Add no
  overwrite flag or interactive prompt. Consumers must use `result.output_path`.
- Make generated filenames portable: drop control characters, replace reserved characters,
  trim trailing dots/spaces, prefix Windows device names, and truncate at a UTF-8 boundary
  within a 255-byte component budget including extension and collision suffix. This is a
  conservative portability policy, not a promise about every filesystem. Reject invalid
  explicit filenames and invalid missing-parent names instead of silently rewriting them.
- Validate path structure, existing ancestors, and obvious Unix read-only permissions during
  planning. A normal run creates missing parents for either output form and opens a private
  sibling temporary file before page processing. That native operation checks actual write
  access; Windows' directory read-only attribute is not treated as an ACL.
- `--dry-run` creates no directories or temporary files. Its read-only checks cannot guarantee
  write access through every ACL, future free space, or that another process will not take
  the planned name. The destination remains a plan, not a reservation.
- Write through `tempfile`, synchronize the completed file, and publish with
  `persist_noclobber`. Never fall back to truncating the destination. If another process
  creates the final path after planning, fail cleanly as `output_write_failed`, preserving
  its file; do not silently change a path already reported in plan events.
- Keep filesystem planning/publication in the CLI's `output` module, separate from the core
  image pipeline. Reuse the existing locked `tempfile` dependency rather than implementing
  platform-specific rename or hard-link operations.

## Consequences and limits

Two sources with the same metadata title no longer choose the same base filename. Repeated
conversions create numbered copies rather than silently replacing a book. This changes the
previous directory-output naming and explicit-overwrite behavior to match the reference;
the README and help describe it. Portable name adjustments are deliberate KCC differences.

The final name is never streamed into directly, and ordinary write/processing/publication
errors clean up the private temporary. `tempfile` documents that no-clobber publication is
not an atomic transaction on every platform: a fallback can leave an extra temporary hard
link. Abrupt termination can also leave a `.mangapress-*.tmp` sibling. No global cleanup scan
or signal handler is added, since another process may still own such a file.

File synchronization does not guarantee directory-entry durability through a power loss,
nor does preflight guarantee space for the completed book. Unsupported filesystem operations
still fail normally. Tests cover partial writes, publication races, cleanup, native paths,
and the terminal/JSON contract without sleeps or oversized fixtures. No pixels change.

## References

- [KCC 12.0.0 output naming](https://github.com/ciromattia/kcc/blob/v12.0.0/kindlecomicconverter/comic2ebook.py#L1067).
- [clig.dev arguments and flags](https://clig.dev/#arguments-and-flags) and [robustness](https://clig.dev/#robustness).
- [tempfile no-clobber publication](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist_noclobber).
- [Windows naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).
