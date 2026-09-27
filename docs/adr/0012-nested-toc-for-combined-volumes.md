# 12. A two-level table of contents for a combined series, EPUB only

## Status

Accepted.

## Context

Mangabound wants an option to bind an entire series into one file instead of one per volume
(Mangabind's new `-combine` mode, see
[Mangabind ADR 0012](https://github.com/gustavommcv/mangabind/blob/main/docs/adr/0012-combine-series-into-one-volume.md)),
while keeping two levels of navigation inside it: a volume entry, its chapters nested underneath -
the same way `docs/adr/0005-mangabind-contract.md` already gives a plain Mangabind volume one TOC
entry per chapter.

Checked directly against this codebase, not assumed, per format:

- **EPUB** — `toc.ncx`/`nav.xhtml` are hand-built XML strings (`ebook/epub.rs`), not produced by an
  off-the-shelf library. Both NCX `navPoint` and EPUB3 nav `<li>`/`<ol>` support nested entries by
  the format's own spec. Building a volume-parent level is a template change to code this project
  already owns end to end.
- **PDF** — the vendored `printpdf` crate's bookmark API (`add_bookmark(name, page)`) is flat;
  every bookmark's `Parent` is hardcoded to the document's single root Outlines dict at serialize
  time. True nested bookmarks aren't reachable through the API this project currently uses.
- **CBZ** — this project writes no chapter-boundary metadata into CBZ output at all today (chapter
  titles only ever become folder names; `ComicInfo.xml` is passed through unmodified with
  `--keepcomicinfo`, never generated). Whether any reader can render a two-level view from a CBZ is
  unverified and out of scope for this decision.

So this decision is EPUB-only. Widening it to PDF or CBZ is real, separate work (a different PDF
library or raw dictionary construction; real-reader verification for CBZ), not something this ADR
blocks or resolves.

Mangabind and mangapress are only ever glued together by an orchestrator (Mangabound) handing one a
file path and reading the other's output - there is no new side-channel manifest here. Volume
boundaries are read from the same place chapter boundaries already come from: each
[`Chapter`]'s `relative_path`. Mangabind's `-combine` output nests
`<volume dir>/<chapter dir>/pNNNN.ext`; a chapter's own parent-of-parent directory *is* its volume.

## Decision

- A new `--nested-toc` flag. `group_into_chapters` is **completely unchanged** - it already keys
  and titles chapters by full relative path (ADR 0005), so a two-level input already produces
  correct, non-colliding `Chapter` values with no code changes.
- `EpubOptions` gains `nested_toc: bool` (default `false`). When true, `build_ncx`/`build_nav`
  group the already-flat chapter list by `relative_path.parent().file_name()` (the volume) - a
  new, purely additive `group_by_volume` helper local to `ebook/epub.rs` - and emit one parent
  `navPoint`/`<li>` per volume with its chapters nested inside, instead of today's flat list. When
  false, output is byte-for-byte identical to before this change.
- `--nested-toc` combined with `--format cbz` or `--format pdf` is refused with a structured error
  (`nested_toc_unsupported_format`) rather than silently ignored or silently falling back to flat -
  the CLI should be honest about what it can't do yet, the same principle Mangabound's own ADR 0009
  applies to its interface.
- `--protocol-version`'s capability list gains `"nested_toc"`. This needs no protocol version bump
  under ADR 0011's evolution rule - capabilities are additive by design.

## Consequences

Mangabound can offer "bind the whole series as one volume" for EPUB without either CLI needing a
new communication channel: Mangabind already reports (and now can nest) the same chapter structure
it always has, and mangapress already owns the TOC-building code end to end. PDF and CBZ stay
exactly as capable (or incapable) as before - this decision doesn't promise them and doesn't block
someone else from separately investigating a PDF library swap or a real CBZ/reader compatibility
test later.
