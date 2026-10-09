# 19. Differences from KCC found by the October 2026 comparison: kept, and left open

Date: 2026-10-09

## Status

Accepted on 2026-10-09 with maintainer approval to merge PR #37. It adds to the table of
[ADR 0013](0013-follow-a-named-kcc-release.md) without changing that record. The maintainer
then decided to follow KCC on every difference left open below ([ADR 0020](0020-follow-kcc-on-the-differences-left-open.md)).

## Context

An analysis compared mangapress 0.7.3 with KCC 12.0.0 on generated books, through the two real
command-line tools (pull requests #30 and #31). Where both tools process the same list of
pages in the same order, the processed pages did not differ in any case tried: stored forms of
a page, page shapes, the gray-or-color decision, option combinations and every built-in
profile. The differences were in which pages there are and in what order, in the contents and
the metadata of a book, in which files are pages, and in how pages are compressed.

Seven of them were bugs and were fixed, each with a case now compared directly in
`tools/parity/books.py`: pages lying directly in a book are listed under its title, not
"Untitled"; a `ComicInfo.xml` that is damaged is a warning, not a failure, and one in UTF-16 is
read; a `.cbz` made by zipping a folder is read as the book it holds (its `ComicInfo.xml`
counts, and its pages lie in the book); and a folder's own pages come before the folders inside
it.

This record is about the others. The IDs are those of `tools/parity/differences.py` and of the
analysis.

## Decision

### Kept on purpose

| ID | KCC 12.0.0 | mangapress | Why |
|---|---|---|---|
| FILE-1 | `.avif` and `.jp2` files are pages | They are left out, counted in the `skipped_non_images` warning; a book of only such files is refused | The image decoders were reduced to the ones the page filter accepts (JPEG, PNG, GIF, BMP, WebP) when the dependencies were cleaned up. To be reconsidered if sources with AVIF pages become common |
| FILE-2 | `.bmp` is not a page | It is | mangapress reads BMP; the coverage map said so, ADR 0013's table did not |
| FILE-4 | A 16-bit gray PNG comes out almost white: only the lowest 255 of 65535 levels survive Pillow's conversion | The levels are scaled and the page looks as drawn | KCC's result is an accident of the conversion |
| FIRST-1 | The book's first page is exempt from cropping when it is a color page; with chapter folders, "first" is the first image of whichever folder the operating system lists first, so it changes from one disk to another | It is always the book's first page | mangapress does what KCC evidently intends, and gives the same book on every system. Not a stable case, so it has none in `differences.py` |
| TOC-3 | A ComicInfo bookmark on a page that is cut in two opens its second half (or the turned copy); a bookmark past the last page stops the run | It opens the first piece; a bookmark past the end is ignored | Opening where the page begins is what a bookmark on it means, and a stray bookmark is no reason to refuse a book |
| TOC-4 | A webtoon book declares no cover | It declares one | A reader shows a cover for the book |
| META-4 | One empty element (`<Summary></Summary>`) makes KCC drop the whole `ComicInfo.xml` | The other fields are used | The rest of the file is still correct |
| META-5 | Text is used as written: line breaks and spaces around a value stay, names are split on comma-space only, entities are decoded a second time | Values are trimmed and entities decoded once | The trimmed value is what the file says |
| ORD-5 | Chapter folders are ordered by an ASCII transliteration of their names, lower-cased, punctuation dropped | By the names as written | KCC's order depends on the platform and the locale (see ORD-1), and a name is not rewritten to order it |
| JPEG-2 | Gray JPEG pages at quality 85 | Lossless pages are identical; at quality 85 the two encoders round differently (up to 1.66 gray levels on dense texture, each equally far from the lossless page), and mangapress's files are 5 to 34% larger (no optimized entropy tables) | Two encoders give the same picture, not the same bytes. The size can be taken up on its own |

### Known and left open

They stay as failing cases in `tools/parity/differences.py`, so that they are not forgotten and
are visible when someone decides.

| ID | KCC 12.0.0 | mangapress | Why it is left |
|---|---|---|---|
| ORD-1 | Inside a folder a name is compared without its extension first: `p01`, `p01 (2)`, `p01-2`; `cover`, `cover2`; `1`, `1.5`, `1.10`, `2` | The extension takes part: `p01 (2)`, `p01-2`, `p01` | KCC's order comes from the `natsort` library's operating-system ordering, which varies with the platform and the locale. To be reconsidered when a real book is put out of order by it |
| ORD-2 | Digits of any script count as numbers (`２` before `１０`) | Only ASCII digits do | The same reason; full-width digits in file names are rare |
| FILE-3 | A PNG cut short becomes a page, blank where its data ends | The run stops with "unexpected end of file" and no book | Stopping names the damaged file; a blank page hides it. To be reconsidered if people want the book anyway |
| META-6 | A negative issue number gives `#-01` | `#0-1` | Cosmetic; no real `ComicInfo.xml` has one |
| JPEG-1 | Color JPEG pages and covers keep chroma at half size in both directions | Chroma at full size: the same quantization tables, files 32% larger on a tinted page and 2.2 times on color halftone, decoded pixels 1.6 to 2.4 levels from KCC's on average | It matters on a color device only, and the encoder in use does not offer the choice |
| CUST-1 | With `--customwidth` or `--customheight` every device gets the 16-level palette | Kindle 1 keeps its 4 levels and Kindle 2 its 15 | No effect on a current device |
| CUST-2 | With a custom size a Scribe or Colorsoft goes back to JPEG quality 85 | It stays at 90 | The same |
| SIZE-1 | PNG pages are written with Pillow's strongest compression | The same pixels in larger files (a synthetic 900x1350 color page: 30 KB against 393 KB) | Measured on a synthetic page only; to be measured on real pages before anything is decided |

## Consequences

Anything on the first table is no longer a defect when it is met again, and does not need a
case: it is as deliberate as the rows of ADR 0013. Anything on the second is a known
difference and not a decision: a case in `differences.py` keeps failing until mangapress follows
KCC there, or until a row moves to the first table with a reason.

The ordering module no longer claims to match both of KCC's orderings: it reproduces
`walkSort()`, and ORD-1 and ORD-2 are the part of the other that it does not.
