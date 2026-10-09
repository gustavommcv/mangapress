# KCC 12.0.0 coverage map

This inventory describes executable checks, not a claim of exhaustive parity. The oracle imports
the separate, pinned KCC checkout; no upstream implementation is copied into mangapress.
See [how to run the checks](README.md) and [ADR 0013](../../docs/adr/0013-follow-a-named-kcc-release.md).

**Direct** means the same synthetic input/options reach both tools and their outputs are compared.
**Rust** means mangapress has regression tests but that behavior is not compared to a live KCC run.
**Extended** cases run before release or through the manual workflow's `extended` input.

The independent [complete-EPUB gate](../epubcheck/README.md) also checks selected device geometry,
nested navigation, bookmark targets, collection metadata and external covers. Those checks do
not turn the Rust-only or excluded entries below into direct KCC comparisons.

| Options / behavior | Direct comparison | Other coverage / limits |
| --- | --- | --- |
| `--profile`, `--format` | Full K11 page matrix; seven representative profiles; EPUB/CBZ books | Rust device-resolution tests also cover PDF plans. Not every profile/format pairing. Auto-format differences are deliberate. |
| `--customwidth`, `--customheight` | KDX/KS3 one-axis override regressions; odd `OTHER` targets; the gray levels of an old Kindle (K1) and the JPEG quality of a Scribe (KS3) with a custom size, pages and cover | Default JPEG/palette behavior with overrides on other device families is not directly covered. |
| `--cropping`, `--croppingpower`, `--croppingminimum`, `--preservemargin`, `--ipc` | Named crop scenarios, 10% cap boundaries; 81% area boundary in extended checks | KCC uses fractions/integers where mangapress uses percentages/enums. Tiny KCC edge-detector crashes are not mangapress failures. |
| `--manga-style`, `--splitter`, `--rotateright`, `--rotatefirst`, `--norotate`, `--maximizestrips` | All spread roles/order; asymmetric odd/square inputs; LTR/RTL and color interactions in extended checks | Upright Kindle EPUB width-cap difference is deliberate; that baseline uses KoC. |
| `--upscale`, `--stretch`, `--wallpaper`, `--blackborders`, `--whiteborders` | Named page/CBZ scenarios | Rust tests cover conflicting border flags and sizing branches; not every interaction is directly compared. |
| `--gamma`, `--autolevel`, `--noautocontrast`, `--colorautocontrast`, `--eraserainbow` | Gray/color scenarios including interacting contrast options | Odd-size rainbow output differences are documented. The new odd-size corpus does not enable rainbow removal. |
| `--forcecolor` | Color/gray page classification and pixels; extended color-PNG book; a book with a page on each side of every boundary of the gray-or-color decision, with and without the option | The decision's inputs are exact PNG chroma. Lossy sources near a boundary depend on the JPEG decoder and are not compared. |
| `--forcepng`, `--force-png-rgb`, `--pnglegacy`, `--noquantize` | Palette/legacy/full-tone CBZ PNG depth/type and pixels; Kindle EPUB PNG/GIF normalized only as documented; extended color PNG | Exact 16/15/4-level dither checks also compare the same input. Not every palette/container pairing. |
| `--jpeg-quality` | Quality 85 book and K11 default 85; extended qualities 1/50/85/90/100 and KS3/KCS defaults 90 | JPEG quantization tables, chroma layout, number of scans, file size (within 5%) and decoded pixels, not identical compressed bytes. Explicit-quality books are grayscale; color JPEG is one drawn set of pages at the default quality, so high-frequency color JPEG at other qualities remains a gap. |
| `--noprocessing` | Ordered original PNG/JPEG/GIF/WebP bytes in CBZ; EPUB in extended checks | Rust codec tests cover BMP, which KCC's CLI does not accept. No input is changed. |
| `--invertdirection`, `--spreadshift`, `--onepagelandscape` | Actual EPUB progression, writing/layout metadata and page sides | Selected combined RTL/inverted/shifted and centered cases, not the whole truth table. Rust EPUB tests cover more combinations and exact property spelling. Center uses the standard `rendition:` prefix, unlike KCC's bare Kindle property (ADR 0018); parity compares the same side, not conformance. KOReader ignores some of this metadata. |
| `--webtoon` | Exact joining/splitting and processed pages; separate KS3 target check | Generated strip geometries, not every gutter/tall-panel arrangement. |
| `--smartcovercrop`, `--coverfill` | Ordinary first-page cover and an RTL wide jacket filled to screen | Other aspect-ratio thresholds and large-cover preshrink difference remain Rust-only/documented. |
| `--title`, `--author`, `--language`, `--metadatatitle` | ComicInfo title/authors/summary; explicit overrides and combine/title-only modes in extended checks | Missing-author fallback and UUID/generator differences are deliberate. Collection metadata and bookmark-based TOC remain Rust-only. |
| `--keepcomicinfo` | Exact source ComicInfo in passthrough CBZ | Other metadata/input combinations remain Rust-only. |
| `--output`, `--nokepub` | Real explicit destinations; extended Kobo EPUB with `--nokepub` | Derived filenames, collision handling and non-overwrite rules use Rust CLI tests; deliberately differ from KCC. |
| Folder sorting, flat chapter navigation | Chapter/page 2 versus 10; actual NCX/nav targets and spine order; a name before the names that continue it, digits of other scripts, extensions of different case, inside a folder and a `.cbz`; the order the real `natsort` gives for 1657 names in 57 groups (`natsort_vectors.py`, checked by a Rust test) | Roman-numeral sorting and repeated chapter-basename handling are not directly compared. Chapter-key collision handling and the order of folder names with punctuation are documented differences (ADR 0019, ORD-5). |
| Layout Mangabound writes (volume folder, chapter folders, `p0001` pages), as a `.cbz` | One volume and a series of two volumes, compared page by page with the navigation and title | Mangabound hands over only this layout; the author is given on the command line, as it does. A case in `books.py` can now be a tree of files, written as a folder or a `.cbz` (`trees.py`). |
| Transparent/indexed inputs and orientation boundaries | RGBA and palette alpha, odd sizes, square/near-square synthetic pages; a book of stored forms (color profiles, four-channel JPEG, 16-bit color PNG, palettes, low bit depths, transparency of every PNG kind, JPEG chroma layouts, GIF, WebP, orientation tags) as gray and as color output; a book of page shapes from one pixel to long strips, with default cropping | 16-bit gray PNG, AVIF and JPEG 2000 pages are not in these books: the tools differ there. Pages cut short are four books of their own (PNG, palette PNG, interlaced PNG, GIF with a transparent color); a truncated WebP is refused by both tools, and a JPEG cut short agrees to within decoder rounding, except inside the first scan of a progressive file. Rust tests cover supported codecs and input rejection. |

## Differences from KCC that were followed

[ADR 0019](../../docs/adr/0019-differences-from-kcc-found-by-the-october-comparison.md) lists the differences
that are kept on purpose. Every other difference found by the October 2026 comparison was followed
([ADR 0020](../../docs/adr/0020-follow-kcc-on-the-differences-left-open.md)), and each is now a direct comparison in
`books.py`: the order of pages beside chapter folders, and of pages whose names continue one another or use digits of
another script; a PNG or GIF cut short; the contents entry for pages directly in a book; a `.cbz` with one top folder
and the `ComicInfo.xml` inside it; a `ComicInfo.xml` that is damaged or in UTF-16; the sign of a negative issue number;
the gray levels and JPEG quality that a custom size gives an old Kindle or a Scribe; and color JPEG pages and covers with
their chroma at half size. Every JPEG and PNG the books compare is also compared in size: a JPEG within 5% of KCC's
(the same quantization tables, chroma layout and number of scans), a PNG at most 1.4 times as large.

## Mangapress-only behavior and excluded KCC features

`--cover`, `--spreads` (explicit label-file selection), `--nested-toc`, `--dry-run`, `--quiet`,
`--list-profiles`, `--json-events`, and `--protocol-version` are mangapress interfaces, not matching
KCC flags. Core and CLI tests cover them; they are not advertised as direct KCC comparisons.
Labelled-spread joining is shared behavior but remains Rust-only: KCC flattens chapter folders,
whereas mangapress preserves them. Covers-folder selection also deliberately differs.

PDF serialization is not compared: Rust tests validate mangapress's PDF output. The oracle never
calls its placeholder PDF or MozJPEG imports. MOBI/AZW3, Panel View, Scribe Amazon page splitting,
light novels, archive/PDF/EPUB input variants beyond mangapress's scope, deletion, batch splitting,
file fusion, target-size limits, MozJPEG and WebP output are excluded/deferred by ADR 0013/0014,
not missing requirements. Safe input limits and link containment follow ADRs 0015/0017 rather
than reproducing unsafe upstream behavior.

The matrix is deliberately selected, not a Cartesian product. Browser/device rendering and
performance are not parity assertions. Add a small failing fixture when a new behavior matters;
do not expand tolerance or silently classify a new difference as intentional.
