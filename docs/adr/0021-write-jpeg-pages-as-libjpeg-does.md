# 21. Write JPEG pages the way libjpeg does, and PNG pages with the strongest compression

Date: 2026-10-09

## Status

Accepted on 2026-10-09. The maintainer decided to follow KCC on every difference
([ADR 0020](0020-follow-kcc-on-the-differences-left-open.md)) and approves this record by
merging the pull request that carries it. It adds a dependency, `jpeg-encoder`, as ADR 0020 said
a record would be needed for.

## Context

KCC saves a JPEG page with Pillow's `optimize=1` and a PNG page with Pillow's `optimize=1`
(`image.py`). Pillow's JPEG encoder is libjpeg: the Huffman tables are built from the picture, a
color picture keeps its chroma at half the width and height (4:2:0), and the file is one scan with
all the components. The encoder mangapress used (the `image` crate's) writes the standard tables
and chroma at full size, and its PNG encoder compresses lightly. The comparison of October 2026
(ADR 0019) found, with the same pixels going in:

- **JPEG-1.** Color JPEG pages and covers: files 32% larger on a tinted page and 2.2 times larger on
  color halftone, and decoded pixels 1.6 to 2.4 levels from KCC's on average.
- **JPEG-2, size.** Gray JPEG pages: files 5 to 34% larger, which is the case of the maintainer's own
  path (a Kindle 11, default options, gray scans).
- **SIZE-1.** Gray and color PNG pages: 3.5 times larger in the median, up to 70 times on a flat page.

## Decision

- **JPEG output is made by `jpeg-encoder` and a recoder of our own** (`crates/mangapress-core/src/jpeg.rs`
  and `jpeg/recode.rs`):
  - `jpeg-encoder` does the transform and quantization. Its quantization tables are the standard
    ones scaled by the quality as libjpeg scales them (the tables of the two files are equal at every
    quality tried), and its forward DCT is the port of libjpeg's integer DCT. Gray pages decode to the
    same pixels as Pillow's (a mean difference of 0.000 to 0.576 levels across the books, 0.000 at the
    median).
  - **The color conversion and the chroma reduction are libjpeg's**, done here and handed to the
    encoder as planes: JFIF's conversion in 16-bit fixed point, and the 2x2 average with the rounding
    bias that alternates 1, 2, 1, 2. With them a color page is 0.005 levels from KCC's at the median
    (0.282 at most); with the encoder's own conversion it was 0.96 and 1.43.
  - **The Huffman tables are built by a recoder** (ITU T.81, Annex K.2): the picture is encoded with
    the standard tables, the symbols are counted, tables that suit them are built, and the same
    symbols are written again. The encoder can build tables itself, but then it writes each component
    as a scan of its own; a decoder in an e-reader that expects libjpeg's single scan may not read
    that, so the file is kept to libjpeg's shape. The recoder reads back only what this encoder
    writes and leaves any other file as it is.
  - Two things the encoder writes differently from libjpeg are corrected so that the files can be
    compared with KCC's: the unused chroma quantization table of a gray page is dropped, and the
    components of a color page are numbered 1, 2, 3 as JFIF and libjpeg do (the decoder the `image`
    crate uses takes 0, 1, 2 for something other than Y, Cb, Cr and shows the wrong colors).
- **PNG pages are written with the strongest compression and adaptive row filters**
  (`png_out.rs`), as Pillow's `optimize=1` does. Palette PNGs already did.
- **The license.** `jpeg-encoder` is `(MIT OR Apache-2.0) AND IJG`: the IJG part is the integer DCT
  it ports from libjpeg through mozjpeg. The IJG license is permissive and asks for a notice in the
  documentation. `about.toml` accepts it for this crate alone, with the notice taken from the
  header of the file that carries it, so that the dependency notices of a release have the text
  and the copyright (checked by `tools/licenses/generate.py`), and `THIRD-PARTY-NOTICES.md`
  has the acknowledgement the license asks for. `cargo audit` finds nothing in the new lockfile.
  The crate has no dependencies of its own.

## Consequences

A gray page is the size KCC's is (0.989 to 1.002 times, 70 pages) and its pixels are KCC's; a color page
and its cover are KCC's in chroma layout, tables, number of scans and, within 0.3 levels, pixels. A
PNG page is at most 1.25 times (gray) or 1.32 times (color) the size of KCC's, and is often smaller; the
rest is the difference between two deflate implementations. A 200-page chapter of a generated mixed
set converts in 3.99 s instead of 3.55 s (12% slower) and makes a book 5.8% smaller. The comparison with
KCC now fails when a JPEG differs from KCC's in its tables, chroma layout, number of scans or size (more than
5%), or a PNG is more than 1.4 times as large.

The bytes of a JPEG are not KCC's: two implementations of the same transform round some
coefficients differently (the pixels differ by at most 0.576 levels on average), and
zlib and the deflate used here do not make the same PNG. What stays different is also what ADR 0019
keeps on purpose.
