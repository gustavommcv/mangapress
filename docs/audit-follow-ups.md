# October 2026 audit follow-ups

The [audit](audit-2026-10.md) is evidence at its stated snapshot, not a list of current defects.
Each topic has regression tests and a separate PR. Merging requires maintainer approval and
verified remote checks for the exact commit, as described in [CONTRIBUTING](../CONTRIBUTING.md).

| Topic | Findings | Status |
| --- | --- | --- |
| Bounded input reads and image checks | 1, 2 | In progress; policy in [ADR 0015](adr/0015-bounded-input-reads.md) |
| Output names, validation, and atomic writes | 3–6 | Not started |
| Symbolic-link policy | 9 | Not started; maintainer decision before implementation |
| Kindle DX and broader parity coverage | 7, 16 | Not started |
| Dependency cleanup | 11 | Not started |
| Distributed dependency license notices | 12 | Not started |
| CI and release checks | 13 | Not started |
| Installer checksums and version selection | 13 | Not started |
| CLI diagnostics, help, and documentation | 8, 10, 15 | Not started |

Finding 18's failure-path tests accompany the relevant fixes. The allocator/distribution
benchmark (14) and the large orchestration refactor (17) are deferred, not resolved.

## Clarification of finding 2

KCC 12.0.0 does override Pillow's default limit: the page parser allows up to 1,431,655,764
pixels, and its webtoon splitter rejects above 1,000,000,000. The audit's statement that
the override appears nowhere in KCC is incorrect. Source references and the maintainer's
choice to retain the larger limit are recorded in ADR 0015. The original report is unchanged.

The input follow-up bounds individual reads and checks image area. It does not establish a
whole-process memory budget; book size, parallelism, and intermediate buffers still matter.
