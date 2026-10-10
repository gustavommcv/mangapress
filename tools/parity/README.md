# Comparing mangapress with KCC

These tools run mangapress and [KCC](https://github.com/ciromattia/kcc) **12.0.0** on the same
synthetic images and compare processed pages and books produced by the real CLIs. Use them for
image-processing, metadata, and book-output changes. They run separately from `cargo test`.
The [coverage map](coverage.md) distinguishes direct comparisons, Rust tests, and remaining gaps.

## Run the comparison

You need the [repository's Rust toolchain](../../CONTRIBUTING.md#setup-and-checks) and Python 3.
From the mangapress repository root, install the
development-only comparison dependencies and clone the reference into a separate folder.
Use a virtual environment if you do not want to change your Python installation:

```sh
python -m pip install -r tools/parity/requirements.txt
git clone --depth 1 --branch v12.0.0 https://github.com/ciromattia/kcc ../kcc-reference
python tools/parity/parity.py --kcc ../kcc-reference
python tools/parity/books.py --kcc ../kcc-reference
```

Sorting and filename sanitization use real `natsort` and `python-slugify` dependencies. Only
unused PDF and MozJPEG imports have substitutes; neither feature is exercised. The full GUI
dependency set and external archive tools are not required. A failing comparison exits nonzero.

- `--profile CODE` selects a built-in device profile; the default is `K11`.
- `--smoke` runs three representative scenarios: default processing, CBZ padding, and color
  output. It excludes the full matrix and exact dither/webtoon checks.
- `--only TEXT` runs scenarios whose names contain the text. Repeat it to select a union.
  A filter matching nothing fails; `--only dither` generates its reference pages even in a
  fresh work folder. It cannot be combined with `--smoke`.
- `--pages DIR` adds your own pages to the default and color-output scenarios.
- `--work DIR` changes the output folder; the default is the ignored `target/parity/`.
- `--extended` adds pre-release boundary/option interactions. `books.py` also accepts it to
  check more metadata modes, devices, color PNG, and JPEG qualities 1, 50, 85, 90, and 100.
  Its ordinary run checks quality 85 and the profile default, alongside the basic book cases.
  `books.py` also accepts `--only TEXT`, repeated for a union; a filter matching nothing fails.

Inspect the generated PNGs from both tools when investigating a difference. Keep private pages
outside the repository; the built-in corpus uses generated images.

For example, check the DX's CBZ behavior or run the small matrix on a Kobo:

```sh
python tools/parity/parity.py --kcc ../kcc-reference --profile KDX --only CBZ --work target/parity/KDX
python tools/parity/parity.py --kcc ../kcc-reference --profile KoLC --smoke --work target/parity/KoLC
```

Use a separate `--work` folder for simultaneous runs. To check the driver's own bookkeeping:

```sh
python -m unittest discover -s tools/parity -p "test_*.py" -v
```

## What is checked

| Property | Expected match |
| --- | --- |
| Detected background and black-background flag | Exact |
| Output page count, order, and page/spread type | Exact |
| Page dimensions and grayscale/color classification | Exact |
| Decoded pixels | Mean difference at most 1.0 gray level, or 1.5 for color |

Palette dithering, webtoon joining, and webtoon splitting have separate pixel-exact comparisons.
The dither checks use 16-, 15-, and 4-level palettes.

Both sides are compared before their JPEG step: KCC's driver stops short of its save, and the
mangapress side is written as lossless PNG (`parity_dump --lossless`) unless the scenario is about
PNG output. What is compared is the processing, not the codec; the codec has its own checks in the
book comparisons below (quantization tables, chroma layout, scans, size, pixels). Most scenarios
come out at a mean difference of 0.00 and none above 0.30; the tolerance is kept wide enough for
the resampler and the dither, not for an encoder. A passing comparison applies to the tested
scenarios; it is not a guarantee for every image, option, or device.

Book comparisons run KCC's actual CLI entry point and the built mangapress executable on small
chapter folders. They read each generated archive and compare:

- ordered page images, geometry, grayscale/color classification, and decoded pixels;
- EPUB title, creators, language, summary, reading direction, layout metadata, and page sides;
- NCX and EPUB3 table-of-contents labels and their actual spine-page targets; the shared
  reader also requires both navigation trees to agree;
- the declared cover and its processed pixels;
- PNG bit depth/color type and JPEG quantization tables, not identical compression bytes;
- exact source-image bytes in passthrough mode, retained CBZ ComicInfo, and unchanged inputs.

Three generated books go beyond the small chapter folders, each as one chapter of lossless
output pages so that reading and processing are compared, not compression:

- **stored forms of a page**: embedded color profiles, four-channel JPEG of both kinds,
  16-bit PNG with color or transparency, palettes with and without transparent entries,
  1-, 2- and 4-bit PNG, gray with transparency, every JPEG chroma layout, progressive JPEG,
  GIF and WebP including two-frame files, content that does not match its extension, and
  orientation tags, which neither tool applies;
- **page shapes**: from one pixel to very long strips, around square, around the ratios where
  a wide page becomes a spread and where a spread is cut in two, and around the screen;
- **the gray-or-color decision**: a page on each side of every boundary of that decision.

Only KCC's documented Kindle GIF versus mangapress PNG difference is allowed between page
codecs in the tested Kindle EPUBs. UUIDs, generator names, timestamps, internal filenames,
and ZIP serialization are not compared. Image tolerances are the same as the page checks.
Page sides compare their meaning after normalizing the `rendition:` prefix: Kindle centered
items use the standard prefixed property, not KCC's undefined bare spelling
([ADR 0018](../../docs/adr/0018-standard-centered-spine-property.md)). Exact spelling has Rust
regression tests. The independent [EPUBCheck gate](../epubcheck/README.md) validates selected
complete mangapress EPUBs; this comparison does not replace conformance validation.
Negative-control tests ensure missing/reordered pages, broken references, changed metadata,
wrong codecs/quality, and altered passthrough bytes cannot produce a successful comparison.

KCC cleans temporary `KCC-*` folders during conversion. Book checks give each subprocess a fresh
temporary root inside its own output folder; they never use the system temporary root. Input
fixtures and generated books remain under `--work` for diagnosis. Temporary conversion files
are removed when each case ends. Repeat runs use fresh case folders, not stale books.

## Limits

The main scenarios use the selected profile. Four explicitly labelled custom-resolution
regressions always use `KDX` or `KS3`; boundary cases use `OTHER` with explicit dimensions.
The upright-spread scenario always uses `KoC` (1072 × 1448),
matching the original Kindle 11 baseline without KCC's deliberate Kindle EPUB cap. These
fixed-profile cases are labelled with their actual code in the output.

Webtoon checks use the selected profile's effective EPUB target, obtained from KCC's own
option resolver, including the Scribe width cap. The exact dither check still
exercises all three palettes (`K11`, `K2`, `K1`), independently of the selected device.

Books exercise selected EPUB/CBZ behavior, not every markup attribute or every option pairing.
The XHTML of a page is pinned by Rust tests, not compared with KCC's: it has one stylesheet rule more than KCC's
([ADR 0023](../../docs/adr/0023-no-line-height-around-a-page-image.md)).
The generated books above are single-chapter folders. A 16-bit gray PNG
and AVIF or JPEG 2000 pages are left out of them because the tools differ there. Pages cut short have their
own books (a PNG, a palette PNG, an interlaced PNG and a GIF); a JPEG cut short is read by both tools, and
agrees to within decoder rounding unless it was cut inside the first scan of a progressive file.
PDF books, labelled-spread joining, ComicInfo bookmarks, external/Covers-folder selection, and
every smart-cover threshold are not directly compared. Relevant Rust tests cover them separately.
Some tiny crop inputs crash KCC's edge detector; crop-boundary fixtures use nonempty edge strips.
See the coverage map for other gaps and
[ADR 0013](../../docs/adr/0013-follow-a-named-kcc-release.md) for deliberate differences, including
PNG rather than GIF for quantized Kindle EPUB pages and the size of upright spreads.

## Automatic checks

[KCC parity](../../.github/workflows/parity.yml) runs on pull requests and pushes to `main`
that change the crates, Cargo manifests/lockfile, comparison tools, or that workflow. It uses
KCC 12.0.0 at commit `f127adbca992456e173d88ada18643eae66802fb` and the dependency versions in
`requirements.txt`: the full Kindle 11 matrix, including the DX and Scribe custom-resolution regressions,
then smoke checks for `K1`, `K2`, `KDX`, `KS3`, `KCS`, `KoLC`, and `RmkPPMove`. This spans all
three grayscale palettes, old Kindle defaults, large screens, color devices, and each family.
A separate `KS3` webtoon check also exercises its format-specific target.
Routine CI adds the small boundary matrix and thirteen CLI-to-book cases.

It runs on Linux and supplements, rather than replaces, the three-platform Rust CI. There is
no schedule or latest-release monitor. `workflow_dispatch` permits a manual run, with an
`extended` checkbox for the pre-release cases; reviewing
a new KCC release remains the contributor's responsibility below. Generated images and KCC's
checkout stay under ignored `target/` and are not shipped in the application.
The release workflow calls this comparison with `extended: true` on each tagged commit before
packaging. This adds three boundary interactions and increases the book matrix to 29 cases.
You can run the same extended checks locally before proposing a release:

```sh
python tools/parity/parity.py --kcc ../kcc-reference --extended
python tools/parity/books.py --kcc ../kcc-reference --extended
```

Changing `rust-toolchain.toml` triggers the PR/main comparison as well.

## Files

- [parity.py](parity.py): scenarios and comparisons.
- [books.py](books.py): actual CLI-to-book comparisons.
- [trees.py](trees.py): the small books `books.py` builds from a list of file names, and the damaged and unusual pages some of them hold.
- [natsort_vectors.py](natsort_vectors.py): the order of file names that the real `natsort` gives, kept in a file the Rust tests read.
- [epub_book.py](../epub_book.py): shared semantic EPUB inspection, also used by the conformance gate.
- [kcc_oracle.py](kcc_oracle.py): calls KCC from its checkout.
- [make_corpus.py](make_corpus.py): generates the test images.
- [book_inputs.py](book_inputs.py): generates the books of stored forms, shapes and decision boundaries.
- [parity_dump.rs](../../crates/mangapress-core/examples/parity_dump.rs): runs mangapress's pipeline
  and writes pages for comparison.

The [licensing boundary](../../docs/adr/0007-gplv3-boundary-kcc-image-rs.md) applies here too.

## Updating the reference

Run the comparison against the proposed KCC tag. Review each difference before changing the
reference: follow the new behavior or record a deliberate difference in ADR 0013. Update the
implementation, regression tests, and `REFERENCE_KCC` in `parity.py` together.
