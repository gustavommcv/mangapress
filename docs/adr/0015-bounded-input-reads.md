# 15. Bound input reads and check image dimensions before decoding

Date: 2026-10-07

## Status

Accepted.

## Context

The October audit reproduced a process abort from a ZIP64 entry declaring 2^62 bytes.
The reader reserved that capacity before reading any data. It also read non-image entries
in full before the CLI discarded them, and had no explicit pixel-area check.

Finding 2 incorrectly states that KCC 12.0.0 never changes Pillow's pixel limit. At tag
`v12.0.0` (`f127adbca992456e173d88ada18643eae66802fb`), its page parser sets
`Image.MAX_IMAGE_PIXELS` to 715,827,882 before opening a page. Pillow's fatal threshold is
twice that: 1,431,655,764 pixels. The webtoon splitter separately sets the warning
threshold to 1,000,000,000 pixels and treats that warning as an error.

The maintainer chose to retain KCC's larger page limit rather than adopt Pillow's smaller
default. A fixed low width or height would also reject legitimate long, narrow strips.

## Decision

- Read at most **256 MiB per source file or uncompressed archive entry**, plus one byte to
  detect an overrun. Reject an oversized declared length before reading, but never use it
  to reserve memory. Check the actual bytes too. The cap applies to metadata, external
  covers, and spread-label files as well as pages, not to the total size of a book.
- The 256 MiB cap is a deliberate difference from KCC. It leaves room for unusually large
  individual pages without letting a single compressed entry consume arbitrary memory.
  It is a named policy constant, not a timeout, and can be reconsidered for a real input.
- In book readers, use the existing page-name rules before opening or decompressing a
  payload. Keep root-level `ComicInfo.xml` and count ignored entries for the existing
  `skipped_non_images` warning. Generic archive/folder readers still return every file,
  subject to the per-file limit.
- Check source image area against **1,431,655,764 pixels** before decoding pixel buffers.
  Use the same check for pages, passthrough mode, covers, labelled spreads, and PDF images.
  Check a merged webtoon strip against **1,000,000,000 pixels** before allocating its canvas,
  since KCC's following split stage would reject it.
- Use `image`'s readers, decoders, and allocation checks; do not introduce another decoder.
  Keep the crate's default 512 MiB decoder allocation limit. Pixel-count limits do not
  replace it, and permitted dimensions alone do not guarantee a successful conversion.
- Keep existing protocol-v1 failure codes and stage context. Oversized entries fail as
  `input_read_failed`; oversized images use the relevant page, cover, spread, or webtoon
  failure. No new CLI flags or framework dependencies are needed.

## Consequences

Malformed ZIP lengths fail normally with a terminal error or JSON error event instead of an
allocation abort. Ignored payloads do not consume decompression time or memory. Accepted
ordinary pages keep the existing image-processing and codec behavior.

These are per-entry and per-image checks, **not a total process memory budget**. The book's
encoded inputs and output still live in memory, pages run in parallel, and RGB conversion
and processing need further buffers. A sufficiently large valid book can still exhaust
memory. Bounding total memory or streaming book assembly is a separate design question.

The image crate documents `max_alloc` as best effort, with support varying by decoder.
Nothing here claims to sandbox an untrusted decoder or to guarantee survival of every
resource-exhaustion input. Symbolic-link policy belongs to the separate audit follow-up.

Regression tests use tiny ZIP64/BMP headers and readers with small test limits, never
multi-gigabyte allocations. The shared fixtures also exercise the real CLI in terminal
and JSON modes. Pixel processing remains checked against the named KCC reference.

## References

- [Pillow decompression-bomb behavior](https://pillow.readthedocs.io/en/stable/reference/Image.html#PIL.Image.open).
- [KCC 12.0.0 page limit](https://github.com/ciromattia/kcc/blob/v12.0.0/kindlecomicconverter/image.py#L154).
- [KCC 12.0.0 webtoon limit](https://github.com/ciromattia/kcc/blob/v12.0.0/kindlecomicconverter/comic2panel.py#L106).
- [image decoder limits](https://docs.rs/image/0.25.10/image/struct.Limits.html).
