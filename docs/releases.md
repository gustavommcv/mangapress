# Release checks

Releases still require maintainer approval. Bump the workspace version in a focused PR,
verify the exact commit's remote checks, and merge it into `main` before pushing its matching
`v<version>` tag. Do not move a published tag or rebuild a different commit under its name.

The tag workflow validates the Cargo version and checks that the tagged commit belongs to
`main`. It then calls the same CI, KCC comparison and [EPUB conformance](../tools/epubcheck/README.md)
workflows used for PRs, on that exact commit, with the extended pre-release parity cases
enabled, before building the four existing release targets. A failing validation, test,
advisory check, notice generation, comparison, EPUB check or build blocks publication.

Only the publication job has repository write permission. It downloads the `mangapress-*`
package artifacts, not the separate CI license reports, generates checksums, and requires
the release tag to exist. Archive names and installer entry points are unchanged.

## 0.7.3 preparation (not released)

This patch candidate contains the October audit fixes and the EPUB follow-ups since 0.7.2.
It adds no new CLI flags or output formats; the JSON protocol remains version 1.
The audit and pre-release PRs are tracked in [audit follow-ups](audit-follow-ups.md).

Draft release highlights:

- Preserve existing outputs, validate portable output names, and stage completed writes.
  Unix output permissions follow ordinary file creation and the user's `umask`.
- Bound individual input reads, check image dimensions before decoding, and keep folder
  links inside the selected input. These checks are not a whole-process memory limit.
- Match KCC 12.0.0's format-specific Kindle DX target and Scribe EPUB width cap.
- Correct centered EPUB spine properties and nested NCX ordering. BMP input still works
  with processing; EPUB with `--noprocessing` now rejects BMP with a recovery message.
- Improve page-failure diagnostics, terminal help and pipe handling. Installers verify
  versioned downloads against release checksums and retain included license notices.
- Expand selected KCC comparisons and add a pinned EPUBCheck gate for 19 complete synthetic
  books, content assertions and failure controls. This is not exhaustive reader coverage.

The package version is prepared, not published. README installer examples intentionally name
the available 0.7.2 release. The reader smoke test below is complete for its stated candidate;
verification of the final release commit's checks remains necessary before a separately approved tag.
Do not update Mangabound's toolchain pin until verified release assets are available.

### Reader smoke test

Use the exact candidate commit in an isolated checkout, build the native CLI with
`cargo build --release --locked -p mangapress-cli`, and record its `--version`. Run the
[complete-book gate](../tools/epubcheck/README.md) to generate fresh books; retain its
printed run directory, logs and `summary.json`. Do not open the intentionally broken
`missing-resource.epub` as a positive case.

In KOReader, inspect the ordinary JPEG and centered PNG books, nested volumes, bookmarks
after RTL splitting, color PNG, mixed-codec passthrough, Unicode/overridden metadata,
external cover, and a large Scribe case. Check actual page rendering, first/next/last pages,
TOC labels and destinations, cover and metadata. Use `cases.py` for the expected page order
and targets; XML assertions alone do not demonstrate reader behavior.

Record the Linux distribution/architecture, KOReader version and build, window or screen
dimensions, relevant reader settings, observed results and screenshots. Preserve personal
books and settings. Classify failures separately from unsupported reader features and
blocked checks; ADR 0013 already records KOReader's handling of spine-placement properties.
Small fixtures do not assess manga quality, and full-size cases stretch synthetic patterns
only to exercise dimensions. If the test uses desktop KOReader, state that clearly: it
does not verify a physical Kindle/Kobo, e-ink refresh or device performance.

### Reader smoke test result (2026-10-08)

Candidate `5416bee8ef4b20938097bfc5d50af5693b826824` was tested on Arch Linux x86_64
with desktop KOReader 2026.07.1, an isolated reader profile and a 536 × 724 viewport
(half the Kindle 11 dimensions, with the same aspect ratio). The native CLI was built
with Rust 1.99.0, not the pinned 1.98.1; the repository's CI remains the authority for
the release compiler.

The gate passed all 19 positive books, both BMP passthrough refusals and the broken-resource
control. Reader checks passed for rendering, page order/counts, navigation targets, covers,
metadata, mixed codecs, color and large/custom dimensions. No reader finding blocked release.
Centered placement and page-side/direction properties were not observable in KOReader,
as already recorded in ADR 0013. Minor punctuation differences in its information screen
were not present in the validated OPF. This was not a physical-device, performance or
real-manga quality test; screenshots and logs were retained by the tester.

The test also found a Unix output-permission regression: staged files retained `tempfile`'s
0600 default after publication. Staging now requests ordinary file permissions through
the existing builder API, allowing the kernel to apply `umask` without changing it.
The temporary and published file share these permissions. CLI regression tests cover
EPUB, CBZ and PDF, explicit file and directory destinations, and ordinary/group-restricted/
owner-only masks. Windows staging and book content are unchanged. Recheck the final
commit's CI, parity and EPUB conformance before tagging.

## Maintenance

- Rust 1.98.1 is the tested minimum and release compiler. The repository toolchain file is
  the source used by rustup in local development, CI, and release builds. Update it and the
  inherited Cargo `rust-version` together; rerun the three-platform checks and KCC comparison.
- External actions are pinned to commit SHAs with release labels. Review upstream changes
  before updating the pins. cargo-about 0.9.2, cargo-audit 0.22.2, and actionlint 1.7.12 are
  build/check tooling, not runtime dependencies.
- EPUBCheck 5.4.0's official ZIP is checksum-pinned in `tools/epubcheck/check.py`.
  Its Java runtime is test tooling only. Review version/digest updates together and run
  both the complete synthetic books and the intentionally broken-resource control.
- The RustSec database is intentionally current at each audit. Tool pins and a lockfile do
  not freeze security findings or GitHub-hosted runner images. Do not treat a database
  fetch failure as a clean audit.

Branch protection, Dependabot updates, and GitHub private vulnerability reporting are
repository settings or separate automation choices, not enabled by these workflow edits.
The [security policy](../SECURITY.md) provides the current private contact. Signing, SBOMs,
build provenance, new platforms, and scheduled parity monitoring remain outside this change.
