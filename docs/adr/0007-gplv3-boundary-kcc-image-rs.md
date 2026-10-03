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
`pipeline/spread.rs`, `rainbow.rs`, `quantize.rs`, and since then
`color.rs`, `webtoon.rs` and `ebook/cover.rs`) — that repetition is
intentional so no future contributor porting new behavior misses this
distinction.

Two later additions stay inside this boundary and are worth naming, since
they look at first as if they might not:

- `tools/parity` (ADR 0013) runs KCC's own code, GPLv3 files included, to
  compare its output with mangapress's. It imports KCC from a checkout the
  user makes and calls it. Nothing of KCC's is copied into this repository
  or shipped in the binary; the check is a development tool, not part of
  the program.
- `resample.rs`, and the dither in `quantize.rs`, reproduce Pillow's
  arithmetic to the bit, because that is what KCC's results are made of.
  Pillow is not KCC and is not under the GPL: its license is permissive,
  and following it closely raises none of the questions above. It does
  ask that its notice travel with the work, which is what
  `THIRD-PARTY-NOTICES.md` is for.

## What this means in practice

Added in October 2026, to say plainly what the decision above does and
does not claim.

**This is not a clean-room reimplementation.** The people (and tools)
that wrote mangapress read KCC's source, GPLv3 files included, to learn
what it does. What the decision keeps out of this repository is KCC's
*expression* — its code, its comments, the way it names things — not
knowledge of its behavior. The claim is "independently written to do the
same thing", not "written without ever seeing it".

**What is taken from the GPLv3 file is facts.** Thresholds, filter
choices, the order of steps, and the device table: each device's name,
screen resolution and gray levels. Module docs name the upstream function
whose behavior they reproduce, as a reference; they are headed "Upstream
reference", not "port", because that is what they are.

**What was checked.** On 2026-10-03, every comment, docstring and string
in KCC 12.0.0's Python files was compared with this repository's Rust
source:

- No run of six consecutive words from `image.py`, `dualmetafix.py` or
  the four headerless algorithm files appears anywhere in it.
- The overlaps that exist are all with ISC-licensed files
  (`comic2ebook.py`, `metadata.py`): the EPUB package's markup, which a
  compatible book has to contain, the names of command-line options, kept
  on purpose so that KCC's users find them, and the wording of a few
  option descriptions. KCC's ISC notice is reproduced in
  `THIRD-PARTY-NOTICES.md` for these.
- A handful of internal names had followed upstream's (two local
  variables, a constant and a function from `image.py`; two functions
  from the headerless crop files). They were renamed.

A check like that finds copied text. It cannot show that no function's
structure follows upstream's too closely; that remains a matter of how
the code is written and reviewed, which is the rule above.

**When adding behavior from `image.py`:** read it to find out what it
does, write down what it does, and implement that — in this codebase's
own structure and names — then check the result against KCC's output
with `tools/parity`, not against KCC's code.
