# 5. The Mangabind chapter-subfolder contract, and fixing a KCC bug while preserving it

## Status

Accepted.

## Context

[Mangabind](https://github.com/gustavommcv/mangabind) delivers each volume
as a `.cbz` containing one subfolder per chapter, named `cNNN - Chapter
Title/`. This works today against upstream KCC because of how
`buildEPUB()` (`comic2ebook.py`) walks the extracted archive:

- `os.walk` is **fully recursive**, not just first-level. For every
  directory containing at least one qualifying image, the first file in
  that directory (after natural sort) becomes a chapter anchor. So "one
  subfolder = one chapter" is only an accurate description for the common
  case of exactly one level of chapter folders — genuinely nested
  sub-chapters would each get their own independent chapter entry, with no
  merging by depth.
- One exception: `getWorkFolder()` strips exactly one enclosing wrapper
  directory if the extracted archive has only a single top-level
  subdirectory — prevents a lone "Series Title/" wrapper from being
  (mis)treated as the one and only chapter.
- Chapter *titles* are looked up from a dict keyed by the **slugified
  basename only**, not the full relative path (`sanitizeTree()` in
  `comic2ebook.py`). Confirmed bug: two different directories anywhere in
  the tree sharing a basename (e.g. two volumes each with an "Extras"
  folder) silently clobber each other's title in the generated TOC.
- File/chapter ordering is natural sort (`natsort`'s `os_sort_keygen`),
  not plain alphabetical — this is why `c001`/`c002`/... zero-padding in
  Mangabind's naming works correctly today.

Verified live (not just read from source): a synthetic two-chapter input
structured exactly like a Mangabind volume (`c001 - Test Chapter One/`,
`c002 - Test Chapter Two/`, each with a couple of pages) produced a
correct `toc.ncx`/`nav.xhtml` with two chapter entries labeled exactly by
subfolder name.

## Decision

mangapress reproduces the behavior Mangabind actually depends on:

1. Recursive walk, one chapter per directory containing qualifying images
   (matches upstream, and Mangabind's real output is one level deep
   anyway, so this is conservative — it also transparently supports
   anyone who hand-organizes nested sub-chapters, which upstream already
   does today).
2. Chapters are keyed by **full relative path**, not basename — fixing
   the same-basename-collision bug rather than reproducing it. This is a
   deliberate, safe divergence from upstream: it can only ever produce
   *more* correct output (distinct titles that upstream would have
   silently merged), never break an input that worked before.
3. Natural sort for both file renaming and final chapter/page ordering.

The single-wrapper-directory unwrap (`getWorkFolder`) is a nice-to-have,
not required for the Mangabind contract itself (Mangabind's own output
never has a spurious wrapper), so it's deferred rather than blocking v1.

## Consequences

Any test fixture exercising EPUB generation should include at least one
case with two identically-named subfolders in different branches, to lock
in the bug fix as a regression test once the EPUB builder exists.
