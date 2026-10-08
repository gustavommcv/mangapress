# Checking complete EPUBs

This development-only gate builds the CLI, generates small synthetic books, and runs the
official [EPUBCheck 5.4.0](https://github.com/w3c/epubcheck/releases/tag/v5.4.0) on each complete
archive. It is independent of KCC and supplements Rust tests and parity comparisons.
Java and Python are not mangapress runtime dependencies; `cargo test` still needs neither.

## Run locally

Use the repository's Rust toolchain, Python 3.11 or newer, and Java on `PATH`.
CI uses Python 3.13 and Eclipse Temurin 21. From the repository root:

```sh
python -m pip install -r tools/book-fixtures-requirements.txt
python -m unittest discover -s tools/epubcheck -p "test_*.py" -v
python tools/epubcheck/check.py
```

The command downloads the official ZIP and verifies its fixed SHA-256 before extracting
the JAR, adjacent libraries and license files. The version, URL and digest live together
in `check.py`. No unverified JAR override or automatic latest-version fallback is supported.
For an offline run, provide the same official ZIP; its digest is still checked:

```sh
python tools/epubcheck/check.py --archive /path/to/epubcheck-5.4.0.zip
```

`--java` accepts a Java executable path. `--work` changes the artifact root; the default
is ignored `target/epubcheck/books/`. Every run creates a fresh directory, with named cases
and exact expected output paths. Old files or an empty case list cannot satisfy the gate.
Books, dry-run/conversion logs, checker JSON/text reports, and `summary.json` remain there
for diagnosis. A download, build, tool, output, content, report or validation failure exits nonzero.

## Selected complete-book coverage

| Cases | Generated output and assertions |
| --- | --- |
| `kindle-jpeg`, `kindle-centered-png` | Kindle 11 (1072 × 1448 target), default grayscale JPEG quality 85, centered PNG, flat chapters |
| `kobo-color-png` | Kobo Libra Colour (1264 × 1680 target), color RGB PNG and series collection |
| `custom-rotated-png`, `custom-even-size` | Odd 127 × 193 target with rotated spread; even 128 × 192 full-size PNG; no Kindle metadata |
| `kindle-four-tone`, `kindle-fifteen-tone` | K1/K2 full-size 600 × 670 PNG and 4/15-shade limits |
| `kindle-dx-bmp-input` | BMP input processed into full-size 824 × 1000 PNG, not the DX's taller CBZ target |
| `scribe-capped`, `scribe-color-capped` | KS3/KSCS full-size 1920 × 2648 PNG and matching Kindle resolution metadata |
| `scribe-custom-uncapped` | KS3 width override: full-size 1986 × 2648 PNG without Kindle resolution metadata |
| `remarkable-full-size` | reMarkable Paper Pro full-size 1620 × 2160 PNG |
| `colorsoft-default-jpeg` | Full-size 1272 × 1696 color JPEG at profile-default quality 90 |
| `nested-volumes-cbz` | Reverse-ordered CBZ, two volumes, repeated chapter basename, exact two-level NCX/nav targets and naturally ordered distinct pixels |
| `bookmarks-after-rtl-split` | ComicInfo bookmark targets after a spread becomes two pages; RTL progression/sides; automatic cover retains the whole source spread |
| `unicode-collection`, `explicit-metadata-and-direction` | Unicode/XML escaping, deduplicated authors, combined title and series refinements; CLI title/author/language overrides; RTL + inverted direction + shifted sides |
| `mixed-codec-passthrough-cbz` | Naturally ordered JPEG/GIF/WebP/PNG bytes retained exactly, including PNG/WebP alpha, from CBZ |
| `external-color-cover` | Explicit cover selection verified by size and its distinct color |

All 19 positive cases run on relevant PRs, `main`, manual runs and release gates; there is no
separate extended mode here. They reuse `tools/book_fixtures.py` and add small scenario-specific
inputs in `cases.py`. Full-size cases enlarge one tiny synthetic page with `--stretch` to exercise
real device dimensions, not to recommend stretching manga. Ordinary small-page cases retain their
own image dimensions; a device target is not a promise that every encoded page fills it.

Each case checks the dry-run and real JSON plan/result against independently specified target
dimensions, output path and page counts. A dry run cannot write a book, and source files,
ComicInfo, external covers and CBZ archives must remain unchanged. The generated archive is read
with the same stdlib-only `tools/epub_book.py` used by parity. Both NCX and EPUB3 navigation must
agree as trees, including each link's actual spine-page index. Assertions check intended metadata,
reading direction, page placement, decoded image geometry, every XHTML image's intrinsic size
(including Kindle's hidden copy), viewport, codec, selected JPEG quantization tables, cover and
series refinements. Nested ordering uses exact synthetic pixels with contrast/gamma/quantization
disabled; passthrough uses exact source bytes. These are not KCC pixel-parity checks.

## Failure policy and limits

The checker receives complete EPUBs, not isolated OPF/XHTML files, with JSON output,
unlimited message occurrences, and `--failonwarnings`. Zero errors, fatal errors and
warnings are required; there is no ignore list or changed message severity.
The exit status and report counters/messages must agree on success. Missing, malformed,
wrong-version, unrelated or pre-existing reports cannot count as a pass.

A separate negative control removes one declared page image from a copy of a validated
book. The original remains intact. The checker must exit with failure and identify the
missing resource (`RSC-001`); an unrelated error or a tool crash is not accepted.
Two additional CLI controls require BMP passthrough into EPUB to fail, for folder and CBZ input,
with the specific processing error and actionable explanation, no successful result and no book.
BMP's normal processed path remains a positive case. These intentional CLI refusals are not books
accepted by EPUBCheck, and are not counted in the 19 validated books.

Unit tests also deliberately change targets, counts, metadata, tree structure, bookmark positions,
viewport/image dimensions, codecs, JPEG tables, colors, covers, collection refinements and source
bytes. They exercise the assertions and failure handling, but do not substitute for the real CLI
and checker run.

The matrix is selected, not every device, flag or combination. Gaps include smart-cover/fill
thresholds, Covers-folder selection, labelled joins, webtoon books, and arbitrary color profiles
or corrupt inputs; relevant Rust/parity tests cover parts of those paths.

EPUBCheck checks format conformance, not processing geometry or reader appearance. The separate
content assertions cover listed geometry/metadata contracts, not KCC compatibility, visual
quality, accessibility completeness, performance or physical-device behavior. Keep those checks
separate, including a reader smoke test before release.

## Automatic checks and maintenance

[EPUB conformance](../../.github/workflows/epubcheck.yml) runs on relevant PRs and `main`
changes, and can be dispatched manually. The release workflow calls the same gate on the
tagged commit before packaging. It runs on Linux; the existing three-platform Rust CI
remains unchanged. On failure, CI retains generated books and reports for seven days,
not the downloaded Java tool or private manga pages.

Review the upstream release before updating the version/digest together. Verify the
official asset digest, rerun the positive cases and negative control, and verify remote
checks for the exact final commit. Download failures are not clean validations. EPUBCheck's
default whole-book rules are used; `-v` is not a whole-book version selector.
See the official [CLI](https://www.w3.org/publishing/epubcheck/docs/cli/) and
[report](https://www.w3.org/publishing/epubcheck/docs/report/) references for the checker contract.
