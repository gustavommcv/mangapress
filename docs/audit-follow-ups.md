# October 2026 audit follow-ups

The [audit](audit-2026-10.md) is evidence at its stated snapshot, not a list of current defects.
Each topic is handled in a separate PR with regression tests. Merging requires maintainer approval and
verified remote checks for the exact commit, as described in [CONTRIBUTING](../CONTRIBUTING.md).

| Topic | Findings | Status |
| --- | --- | --- |
| Bounded input reads and image checks | 1, 2 | In progress; policy in [ADR 0015](adr/0015-bounded-input-reads.md) |
| Output names, validation, and atomic writes | 3–6 | In progress; policy in [ADR 0016](adr/0016-safe-output-planning-and-publication.md) |
| Symbolic-link policy | 9 | In progress; approved containment policy in [ADR 0017](adr/0017-folder-links-stay-inside-the-input.md) |
| Kindle DX and broader parity coverage | 7, 16 | In progress; format-specific target and [multi-profile comparison](../tools/parity/README.md) |
| Dependency cleanup | 11 | In progress; unused crates removed and image codecs restricted to the supported page formats |
| Distributed dependency license notices | 12 | In progress; target-specific source notices generated and checked for release archives |
| CI and release checks | 13 | In progress; pinned tools/actions, locked checks, advisory checks, and shared release gates |
| Installer checksums and version selection | 13 | Not started |
| CLI diagnostics, help, and documentation | 8, 10, 15 | Not started |
| Final pre-release parity review | Follow-up to 16 | Planned after the audit PRs, before the next release |

Finding 18's failure-path tests accompany the relevant fixes. The allocator/distribution
benchmark (14) and the large orchestration refactor (17) are deferred, not resolved.
The parity follow-up runs on relevant PRs/main changes. A schedule and latest-KCC-release
monitor are not part of this change; new reference releases still require manual review.

## Additional Scribe difference found during the follow-up

The initial `KS3` smoke check passed default processing and CBZ padding, but failed color output
on a rotated spread: KCC 12.0.0 produced 1920 × 2604 pixels, mangapress 1952 × 2648.
KCC's `checkOptions()` caps the width of an unmodified Scribe profile at 1920 for its
Kindle EPUB/MOBI path. Mangapress had used the full built-in width; this was an unintentional
difference, not one of ADR 0013's accepted exceptions.

The maintainer chose to reproduce the cap for EPUB. The shared target resolver and
EPUB metadata now use it; CBZ/PDF and custom dimensions keep their full target. Tests
cover both `KS3` and `KSCS`. This is a compatibility fix under ADR 0013, not a new
deliberate difference. The original audit remains unchanged.

## Clarification of finding 2

KCC 12.0.0 does override Pillow's default limit: the page parser allows up to 1,431,655,764
pixels, and its webtoon splitter rejects above 1,000,000,000. The audit's statement that
the override appears nowhere in KCC is incorrect. Source references and the maintainer's
choice to retain the larger limit are recorded in ADR 0015. The original report is unchanged.

The input follow-up bounds individual reads and checks image area. It does not establish a
whole-process memory budget; book size, parallelism, and intermediate buffers still matter.

## Dependency cleanup scope

Remove unused `imageproc` and `slug` dependencies and enable only JPEG, PNG, GIF, BMP, and WebP
in `image`. Regression tests cover supported inputs, book outputs, and rejection of an
unsupported image disguised with a supported extension, including passthrough mode.
Re-run the full named KCC comparison. Dependency advisory and license checks belong to the
later CI/release and distributed-notices topics, rather than a second policy in this PR.

## Distributed-notices scope

Generate source license texts and credits from the locked CLI graph for all four release
targets, including bundled native-library notices. Reuse cargo-about in CI and release
packaging, and reject generic fallback text that loses copyright attribution. Build
dependencies are included conservatively, not claimed as code present in the executable.
The original KCC/Pillow notices and mangapress's own license remain unchanged.

The installer follow-up must also preserve these files after extraction: Windows already
keeps archive contents, but the Unix installer currently moves only the executable.
Do not consider that installation path covered by archive generation alone.

## CI and release scope

Pin Rust 1.98.1 in the repository toolchain file and inherit that tested minimum in both
packages. Pin external actions by commit, use locked Clippy/tests, and run cargo-audit
against the current RustSec database. actionlint checks workflow syntax; regression tests
cover mismatched release tags. The tag workflow requires a matching Cargo version and a
commit merged into `main`, then reuses CI and KCC parity before packaging or publication.

Repository protection and update bots remain owner decisions, not changes silently made
through the API. No signing, SBOM, provenance, new platform, or scheduled comparison is
added. See [release checks](releases.md) and the [security policy](../SECURITY.md).

## Final pre-release parity review

After the audit PRs, review the comparison against KCC 12.0.0 before the next release:

1. Inventory relevant options shared by both tools. Distinguish direct comparisons, Rust-only
   tests, untested behavior, and deliberate differences documented in ADR 0013. KCC-only
   features are not requirements for mangapress.
2. Fill meaningful gaps: PNG variants, JPEG quality, passthrough, reading direction, custom
   dimensions, and a small selection of interacting options. Extend the synthetic corpus
   with transparency, odd dimensions, asymmetric pages, and threshold-boundary cases.
3. Add a few actual CLI-to-book comparisons for page order, covers, navigation, and metadata.
   Compare the books' meaning, not identical compressed bytes or timestamps.
4. Reuse the existing comparison tools. Keep routine CI small and use a broader matrix before
   release; report the tested coverage and remaining gaps without claiming exhaustive parity.

This is the final planned block, not part of the dependency cleanup. Preserve accepted
differences rather than changing output merely to make a comparison pass.
