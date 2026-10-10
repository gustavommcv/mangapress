# 24. `--format auto` gives CBZ, and PDF only for reMarkable

Date: 2026-10-10

## Status

Proposed on 2026-10-10. The maintainer approves this record by merging the pull request that
carries it. It adds a difference to the table of [ADR 0013](0013-follow-a-named-kcc-release.md) and
replaces its row for `--format auto`.

## Context

`--format auto` picked CBZ for the four oldest Kindle profiles (`K1`, `K2`, `K34`, `KDX`), PDF for
reMarkable, and EPUB for every other profile. Those are KCC's own choices, except that a Kindle got
EPUB where KCC goes on to MOBI (ADR 0013, ADR 0008).

The reading pipeline this tool exists for ends in KOReader (ADR 0008), and ADR 0008 holds that
KOReader reads a fixed-layout EPUB well. Reading on a Kindle (2024) with KOReader 2026.07.2 showed
otherwise. The Mangabound record that found it, with its sources, is
[Mangabound ADR 0045](https://github.com/gustavommcv/mangabound/blob/main/docs/adr/0045-cbz-is-the-format-a-first-launch-starts-with.md);
the findings that bear on this tool are:

- **KOReader's own guidance is CBZ.** Its user guide recommends CBZ for manga, Kindle Comic
  Converter's FAQ says "CBZ for KOReader", and the KOReader maintainers say that EPUB is the wrong
  format for image-heavy content ([koreader#9163](https://github.com/koreader/koreader/issues/9163),
  [#13109](https://github.com/koreader/koreader/issues/13109)).
- **KOReader lays an EPUB page out as reflowable text.** It ignores the fixed-layout viewport and
  keeps its own margins and status bar around the image, so a page is drawn smaller than the screen:
  1002x1354 of 1072x1448 with its default settings, measured in KOReader's own engine
  ([ADR 0023](0023-no-line-height-around-a-page-image.md) gets 12 px of that back, not the rest). A
  CBZ page has no margins, and this tool pads it to the device's screen.
- **An EPUB has no way to fill the width in KOReader.** There is no fit-to-width zoom for one; a CBZ
  has it.

What EPUB still has that a CBZ written by this tool does not: a table of contents with one entry per
chapter, an author inside the file, and the two-level table of contents of `--nested-toc` (ADR 0012).

Mangabound already starts on CBZ and passes `--format` on every run, so it is not affected by the
default. A person at a terminal who leaves `--format` out is.

## Decision

- **`--format auto` gives PDF for the reMarkable profiles and CBZ for every other profile.**
  `--format epub`, `--format cbz` and `--format pdf` are unchanged.
- **`--nested-toc` keeps working without `--format`.** A two-level table of contents exists only in
  EPUB (ADR 0012), so with `--nested-toc` and `--format auto` the format is EPUB, on every profile.
  Until now the four oldest Kindles and the reMarkable were refused here, because their automatic
  format was not an EPUB. `--nested-toc` with an explicit `--format cbz` or `--format pdf` is still
  refused.
- **The row of ADR 0013's table for `--format auto` is replaced by this one.** The difference from
  KCC is now: KCC writes MOBI for the other Kindle profiles and EPUB for the rest; this tool writes
  CBZ.

## Alternatives considered

- **CBZ for Kindle profiles only, EPUB for Kobo and the rest.** Nothing in the reasons above is
  specific to a Kindle: KOReader also runs on Kobo, and a Kobo's own reader opens CBZ (not tested
  here). A rule with an exception for each brand is also harder to say than one with a single
  exception, the reMarkable, whose PDF is the format its own reader is built for.
- **Leave `auto` as it is, and only tell people to pass `--format cbz`.** The default is what
  someone who has not read the documentation gets, and for the reader this tool is written for the
  default is the worse choice.

## Consequences

- A command line without `--format` writes a CBZ for most profiles, where it wrote an EPUB. The
  book has no chapter navigation and no author inside it; `--format epub` brings both back. This is
  a change of default in a 0.x tool, and the release notes of the first release that carries it say
  so.
- The help text, the README and `tools/parity/coverage.md` say what `auto` means now. The parity
  comparisons name their format on every run, so they are not affected.
- If measuring a CBZ on the device ever gives a reason to prefer EPUB again, this record is where
  the default is changed back.
