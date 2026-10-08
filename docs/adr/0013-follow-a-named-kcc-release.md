# 13. Follow a named KCC release exactly, and leave out what exists only for Amazon's converter

## Status

Accepted.

## Context

mangapress was written by reading KCC at one research commit (`ea532c7`, between 11.2.0 and 12.0.0)
and describing what it does (ADR 0007). Each stage was tested on its own and spot-checked against
KCC's output. Nothing compared the two tools' pages wholesale, and "KCC" was not pinned to a
release.

That let differences through that nobody had chosen. The one that surfaced first was visible on a
device: pages came out 11-15% wider than drawn in KOReader. mangapress sized each page's image as a
block in percentages and relied on the fixed-layout viewport; KOReader's engine ignores that
viewport and stretched the block to the screen. KCC's markup — an inline image with its pixel width
and height, in a centered block — does not have the problem. The files were fine; the page markup
was not KCC's, for no reason.

An audit against KCC 12.0.0, stage by stage, then found more of the same kind:

- Grayscale conversion used the `image` crate's channel weights, not Pillow's.
- Whether a page counts as color steers autocontrast in KCC even for grayscale output; here it did
  not.
- Dithering was the textbook Floyd-Steinberg. Pillow's own (which is what KCC's quantization is)
  put 16-37% of a real page's pixels on a different gray level.
- Resizing used the `image` crate's resampler, which evaluates the same filters as Pillow in
  floating point rather than 22-bit fixed point. Close everywhere, and several gray levels off on
  fine hatching when cropping to fill, where Pillow resamples from a fractional box.
- The EPUB package differed in details readers act on: which side of a two-page view each page
  takes, and the cover (the processed first page, instead of a cover made from the untouched
  image — so a first page that was split in two gave half a cover).
- KCC itself moves between releases: 11.2.0 and 12.0.0 crop 25 of 432 pages of one real volume
  differently. "The same as KCC" means nothing without saying which.

Separately, mangapress's own purpose had sharpened: an independent, light command-line tool that
other software is composed from, for books read in KOReader. Not a stage of one pipeline only, and
not a front end for Amazon's own reader (ADR 0008).

## Decision

**The reference is a named KCC release — currently 12.0.0.** Given the same pages and the same
options, mangapress produces the pages that release does: the same detected background, the same
pieces in the same order, the same sizes, the same pixels.

**This is checked, not asserted.** `tools/parity` runs KCC's own code from a checkout next to
mangapress's real pipeline, over a procedurally drawn corpus and optionally over the user's own
pages, and fails on any difference. It imports KCC and calls it; nothing of KCC's is copied into
this repository, so ADR 0007's boundary holds. Real pages are never committed. The EPUB package
and the cover, which the check does not cover, are pinned by unit tests whose expected values came
from KCC's own functions.

**Where KCC's result is really Pillow's, Pillow's arithmetic is reproduced:** gray conversion,
the RGB/YCbCr tables, the palette dither, and the resampler (`color.rs`, `quantize.rs`,
`resample.rs`). The `image` crate's equivalents agree with Pillow only approximately, and
approximately is what let the differences above hide. Pillow's license is permissive, unlike the
two GPLv3 files ADR 0007 is about.

**A difference from the reference is a defect unless it is on this list.** Each entry is
deliberate, and is also written down where the code makes it:

| Difference | Why |
|---|---|
| No MOBI/AZW3 output | ADR 0008. |
| No Panel View, no page splitting for the Kindle Scribe | They exist for Amazon's reader and converter. |
| `--forcepng` writes PNG on every device; KCC writes GIF for a Kindle profile's EPUB | Same pixels, a smaller file (7% on a real chapter), one dependency fewer. The GIF is for Amazon's converter. |
| A spread kept upright (`--norotate`) may be two screens wide on every device; KCC caps it at 1920x1920 for a Kindle profile's EPUB | The cap is the converter's limit. KOReader's engine has none: checked at a Kindle Scribe's resolution, where pages 2480 pixels tall and a spread 2003 wide are drawn undistorted. |
| `--format auto` gives a Kindle EPUB | KCC goes on to MOBI. |
| Only `.cbz` and folders are read | CBR, CB7, PDF and EPUB input need external tools or large libraries; one binary with nothing else to install is the point of this project (ADR 0001, 0002). |
| One input file or uncompressed archive entry is limited to 256 MiB | Bounds an individual read; not a book-size limit. The pixel-area check retains KCC's larger page limit (ADR 0015). |
| Folder links are followed only to regular files inside the selected input; rejected links produce warnings | Prevents unintended inclusion of external images and directory cycles. The selected root may itself be a link (ADR 0017). |
| Chapters are keyed by their full path | Fixes a title collision in KCC (ADR 0005). |
| A cover in the `Covers` folder can be matched by name; `Covers` is not counted as a book | Position alone hands a book without a cover its neighbor's, and KCC counts the folder itself when the input is a folder. |
| An existing output is preserved with `… (mangapress)`, then numbered suffixes; `.kepub.epub` stays intact | KCC's non-overwriting rule, under this tool's name, without duplicating the compound extension (ADR 0016). |
| Generated output names are made portable and fit a 255-byte UTF-8 component budget; invalid explicit names fail early | Avoids reserved Windows names and unwriteable outputs. Book metadata is not changed (ADR 0016). |
| `--croppingminimum` is a percentage (`90`); KCC's is a fraction (`0.9`) | The same threshold, written the way `--preservemargin` already is in both tools. The option names need not be KCC's twin where the behavior is. |
| Pages labelled as the halves of a spread (`--spreads`, or KCC's `<input>.json`) are joined without flattening the book; KCC drops every chapter when it joins | The table of contents survives. For two pages of the same size the joined image is KCC's, pixel for pixel. |
| Two labelled pages of different sizes are placed side by side whole; a label that cannot be used is skipped with a warning | KCC cuts or overlaps the pages in the first case and stops with an error in the second. |
| The book's identifier is derived from the book (a version 5 UUID); KCC draws a random one per conversion | Converting the same book again gives the same identifier, which is what EPUB 3 asks of one. |
| With no author anywhere, the author is `Unknown`; KCC writes `KCC` | The converter is not the author. |
| The rainbow eraser differs by a few gray levels at an odd width or height | KCC rebuilds the spectrum assuming an even width. No built-in profile is odd; only a custom resolution reaches it. |
| The cover is resampled in one pass | Pillow's `thumbnail()` pre-shrinks very large images first. Not reproduced; a known, small difference rather than a chosen one. |

**Options KCC has and mangapress does not:**

- `--filefusion`, `--batchsplit`, `--targetsize`: grouping and splitting volumes is
  [Mangabind](https://github.com/gustavommcv/mangabind)'s job, and splitting would break "one
  input, one book", which the machine protocol (ADR 0011) relies on.
- `--delete`: a tool that others drive should not delete its source.
- `--lightnovel` and the PDF/EPUB input options: follow from the input formats above.
- `--mozjpeg` and `--webp`: smaller pages. Wanted, not yet — both are in ADR 0014.

**When KCC makes a release**, check out its tag and run the parity check. Every difference it
reports is either a change in KCC to follow — code, tests, and `REFERENCE_KCC` in
`tools/parity/parity.py` — or a deliberate difference to add to the table above.

**Three options are written into the EPUB as KCC writes them and do nothing in KOReader:**
`--spreadshift`, `--onepagelandscape` and `--invertdirection` set which side of a two-page view
each page takes and the direction pages turn. KOReader's engine does not read those properties
(checked: the same book renders identically with and without each of them, and the page-turn
direction there is a setting of the reader's own). They are kept for the readers that do — Kobo's
and Kindle's own, and EPUB readers in general.

## Consequences

Anything that changes pixels under `crates/mangapress-core/src` needs the parity check run before
it is merged. It is not part of `cargo test`: it needs Python with Pillow and NumPy and a KCC
checkout, and takes about five minutes.

Reproducing Pillow exactly commits this project to Pillow's behavior where KCC inherits it,
quirks included (the dither's row-end slip is reproduced on purpose; see `quantize.rs`). It did
not cost speed overall: against v0.6.0, on a 432-page volume on a four-core laptop, the work done
since this decision made a conversion 16% slower when no page needs resizing (8.7 s to 10.1 s)
and 22% faster when every page is enlarged (15.7 s to 12.2 s).

Options left out are left out for a reason that can stop holding. A request for one is answered by
that reason, not by "KCC has it".
