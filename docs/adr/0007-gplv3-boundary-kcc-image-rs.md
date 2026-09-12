# 7. Treat KCC's `image.py` and `dualmetafix.py` as specification, not source to port

## Status

Accepted.

## Context

Upstream KCC's `LICENSE.txt` states ISC for the repository as a whole, and
most `.py` files carry a matching ISC header. Two files are a real
exception, confirmed by reading their actual headers (not inferred from
the top-level LICENSE file):

- `kindlecomicconverter/image.py` — the single most important file to
  reference for this rewrite (device profiles, crop/resize/gamma
  orchestration, spread-split thresholds) — carries a **GPLv3-or-later**
  header, copyrighted by Alex Yatskov (2010), Stanislav "proDOOMman"
  Kosolapov (2011), Alberto Planas (2016), plus the two ISC-era
  maintainers.
- `kindlecomicconverter/dualmetafix.py` (the MOBI/EXTH binary patcher,
  currently irrelevant per `0006-mobi-azw3-deferred.md` but noted for
  completeness) also carries a GPLv3-or-later header.

Additionally, four newer algorithm files (`common_crop.py`,
`inter_panel_crop_alg.py`, `page_number_crop_alg.py`,
`rainbow_artifacts_eraser.py`) carry **no license/copyright header at
all** — presumably falling under the repo-level ISC `LICENSE.txt` by
default, but not explicitly stated per-file, so their provenance is
ambiguous.

## Decision

mangapress is dual MIT/Apache-2.0 (`0004-license-dual-mit-apache.md`), a
permissive license incompatible with directly incorporating GPLv3 code.
So, specifically for `image.py` and `dualmetafix.py`:

- Treat the algorithms, thresholds, and structure documented from reading
  them (aspect-ratio cutoffs, filter choices, palette definitions, the
  `splitCheck()` decision tree, etc. — facts/ideas, not copyrightable
  expression) as a specification to reimplement independently.
- Do not copy or closely translate their actual code, comments, or
  variable-naming structure.
- If a future contributor wants to port logic from these two files more
  directly than "reimplement from documented behavior," that specific
  code must be treated as GPLv3 and isolated (e.g., an optional,
  separately-licensed component), not merged into the MIT/Apache-2.0
  core.

For the four headerless algorithm files, default to the same
conservative treatment (reimplement from documented behavior, don't
port directly) until their licensing is clarified, rather than assuming
the repo-level ISC notice definitely covers them.

## Consequences

Every module doc comment in `mangapress-core` that ports KCC behavior
should note which upstream file it's based on and flag GPL-boundary files
explicitly (already done for `resize.rs`, `contrast.rs`,
`pipeline/spread.rs`, `rainbow.rs`, `quantize.rs`) — that repetition is
intentional so no future contributor porting new behavior misses this
distinction.
