# 18. Use the standard EPUB property for centered spine items

Date: 2026-10-07

## Status

Proposed; awaiting maintainer review.

## Context

ADR 0013 follows KCC 12.0.0's page-side assignments and markup. For Kindle profiles,
both tools write `page-spread-center` when `--onepagelandscape` centers every page or
when a double-page spread is kept as a rotated copy. EPUBCheck 5.4.0 rejects this
undefined property with `OPF-027` in both tools' complete synthetic EPUBs.

The EPUB specification defines `rendition:page-spread-center`. The older, unprefixed
`page-spread-left` and `page-spread-right` are also defined; a bare center property is
not. Matching the reference's invalid spelling does not establish EPUB conformance.

## Decision

- Write `rendition:page-spread-center` on centered spine items for every device family.
- Retain Kindle's unprefixed left/right properties and `linear="yes"`. Other families
  keep their existing prefixed left/right properties.
- Keep page-side selection, progression direction, page order, image bytes, dimensions,
  cover, XHTML and navigation unchanged. Only the centered Kindle spine property's
  spelling differs from KCC 12.0.0.
- Compare page-side meaning with KCC, as the existing book comparison already does
  for the `rendition:` prefix. Rust regression tests enforce the exact output spelling;
  this normalization is not an EPUB conformance check or permission to ignore errors.

## Consequences and limits

This is a narrow exception to ADR 0013, not a new EPUB implementation or a change to
its named KCC reference. The original accepted record remains unchanged. No new flag,
runtime dependency, or protocol version is needed.

Tests cover mixed ordinary/rotated pages and `--onepagelandscape`, both reading
directions in the core, and JPEG/PNG books from the CLI on Kindle 11, Scribe 3, Kobo,
reMarkable and an odd custom target. Left/right properties and page order remain
asserted. A negative control ensures parity still rejects a different page side.

ADR 0013 records that KOReader ignores these spine placement properties. No new
physical-device result is claimed here. Adding EPUBCheck to CI and broader book
coverage are separate follow-ups; local validation does not replace remote CI.

## References

- [EPUB 3.3 spread placement](https://www.w3.org/TR/epub-33/#spread-placement).
- [EPUBCheck message reference](https://www.w3.org/publishing/epubcheck/docs/messages/).
- [Named KCC reference and exceptions](0013-follow-a-named-kcc-release.md).
