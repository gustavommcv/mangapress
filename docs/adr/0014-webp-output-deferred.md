# 14. Defer WebP output, and smaller JPEG

## Status

Deferred. Not implemented; a "not yet", decided on 2026-10-03 after the measurements below.

## Context

WebP pages were asked for with one case in mind: older Kindles running KOReader, with about 3 GB
of storage, where the size of each volume decides how much of a series fits. The wish was for the
highest quality possible, not merely the smallest file.

KCC 12 has it as `--webp`: lossy WebP in place of JPEG, at the JPEG quality setting, and lossless
WebP in place of PNG. It ignores the option for a Kindle profile's EPUB and for PDF. Its own note
on the option says the gain is large for color pages, and small for black-and-white ones compared
with PNG, at a higher cost in resources.

### What was measured

108 pages — every fourth page of a real 432-page volume — converted by mangapress for the Kindle 11
profile with upscaling, which gives grayscale pages of 965x1448. The reference for fidelity is the
processed page before any lossy step (`--forcepng --noquantize`). The JPEG is mangapress's own
output; WebP was encoded with libwebp through Pillow 12.3, method 6. "Off by half a step" is the
share of pixels more than 8 levels (of 255) from the reference, half the distance between two of a
16-level screen's grays. Decoding was timed with Pillow on a laptop (Ryzen 5 3500U).

| Format | KB per page | vs today's JPEG | PSNR | SSIM | Off by half a step | Decode |
|---|---|---|---|---|---|---|
| JPEG, quality 85 (today) | 252 | — | 40.4 dB | 0.9894 | 1.67% | 13 ms |
| JPEG, quality 85, optimized and progressive | 237 | −6% | 40.5 dB | 0.9894 | 1.63% | 38 ms |
| WebP lossy, quality 75 | 118 | −53% | 39.1 dB | 0.9876 | 2.63% | 53 ms |
| WebP lossy, quality 80 | 136 | −46% | 40.8 dB | 0.9906 | 1.19% | 55 ms |
| WebP lossy, quality 85 | 157 | −38% | 42.7 dB | 0.9931 | 0.34% | 61 ms |
| WebP lossy, quality 90 | 189 | −25% | 45.2 dB | 0.9954 | 0.02% | 65 ms |
| WebP lossy, quality 95 | 239 | −5% | 48.2 dB | 0.9971 | 0.00% | 74 ms |
| PNG, 16 levels dithered (`--forcepng`) | 274 | +9% | | | | 22 ms |
| WebP lossless, 16 levels dithered | 224 | −11% | | | | 36 ms |
| PNG, 8-bit, lossless | 505 | +101% | exact | exact | 0% | 36 ms |
| WebP lossless, 8-bit | 443 | +76% | exact | exact | 0% | 46 ms |

(The dithered rows carry no fidelity figures: dithering is noise added on purpose, and comparing
it pixel by pixel with the page it came from says nothing about how it looks.)

What the numbers say:

- **Lossy WebP is the only one of these that saves real space.** At quality 80 it matches today's
  JPEG in fidelity at a little over half the size. At 85 and 90 it is both smaller and closer to
  the original than today's JPEG. For a 200-page volume: 49 MB as JPEG, 31 MB at quality 85, 37 MB
  at quality 90. In 3 GB, about 62 volumes become about 100, or 83.
- **The saving varies by page**: at quality 85, from 21% to 57% smaller, median 38%.
- **Lossless is not a way to save space against JPEG.** It is for the dithered mode, where lossless
  WebP is 18% smaller than the palette PNG — the modest gain KCC's note describes.
- **Decoding costs more.** On a desktop, lossy WebP took four to six times as long as baseline
  JPEG to decode. The devices that would gain the most storage are the ones with the slowest
  processors.

### What is known besides

- KOReader has read WebP since August 2022 (its change #9402). Its engine, driven headlessly from
  a current desktop build, draws lossy WebP pages correctly.
- The `image` crate's WebP support, already in the dependency tree, decodes, and encodes lossless
  only. Lossy encoding means either libwebp — C, compiled into the binary on every release target —
  or one of the pure-Rust encoders that have appeared (`zenwebp`, `webp-rust`, `gamut-webp`). None
  of those was tried; the figures above are libwebp's.

### What was not measured

- Decoding time, and so page-turn time, on a real e-reader — least of all an old one.
- How the pages look. The fidelity figures are arithmetic, not a judgment made on an e-ink screen.
- Which readers other than KOReader open WebP inside an EPUB.

### The other way to a smaller file: the same JPEG, packed better

KCC also has `--mozjpeg`, which rewrites each JPEG without changing a pixel — optimized Huffman
tables and progressive scans — through a C library. Its window promises a file 10-20% smaller for
twice the processing time. That was not measured. What was, on the same pages, is what a
pure-Rust encoder could do along the same lines:

- optimized Huffman tables alone: about 3% smaller (12 pages), and no slower to decode;
- optimized and progressive: 6% smaller (the table above), and about three times as slow to
  decode on a desktop.

It needs no reader support and loses nothing, which WebP cannot say; it also saves a small
fraction of what WebP does.

## Decision

Not now. WebP output is a future feature, and not part of the release that follows the KCC 12
parity work (ADR 0013). The same goes for a smaller JPEG: deferred with it, as the cheaper and
smaller of two answers to the same wish.

The saving is real and the fidelity is not in doubt. What is undecided is everything that makes
it a feature rather than a measurement, and each of these needs an answer first:

1. **The encoder.** One binary with nothing else to install is this project's point (ADR 0001,
   0002). A C library compiled in keeps that for the user but adds a C toolchain to every release
   build. A pure-Rust encoder keeps the build simple but has to be shown to come close to libwebp
   at the same quality, on pages like these, before the figures above can be claimed for it.
2. **What the quality setting means.** "Highest quality possible" can be a fixed high setting
   (quality 90: closer to the original than today's JPEG by every measure, and a quarter smaller),
   or the fidelity of today's JPEG at the smallest size (quality 80), or the existing
   `--jpeg-quality` reused as KCC reuses it.
3. **The cost on the device.** Page turns on an old Kindle, timed, with the same chapter as JPEG
   and as WebP. If turning a page becomes noticeably slower, the storage is paid for in the wrong
   currency.
4. **The scope.** Whether `--forcepng` pages become lossless WebP too, as in KCC. PDF cannot hold
   WebP; KCC ignores the option there.
5. **Which KOReader is required**, and what happens in a reader that cannot open the pages.

## Consequences

mangapress has no `--webp`, and ADR 0013 lists it among KCC's options that are missing on purpose.

The measurement is reproducible from what is described above: convert the same pages three ways
(default, `--forcepng`, `--forcepng --noquantize`), re-encode the third, and compare with it. The
pages themselves are not in the repository and must not be (see `tests/fixtures/README.md`).

Revisit this ADR, rather than writing a new one, when the feature is taken up — as ADR 0010 was
for color output.
