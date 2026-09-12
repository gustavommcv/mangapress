# 3. Two-crate workspace: `mangapress-core` (lib) + `mangapress-cli` (bin)

## Status

Accepted.

## Context

KCC's own `kindlecomicconverter/` package is modular at the file level:
I/O (`comicarchive.py`, `comic2ebook.py`), each image algorithm isolated
(`image.py`, `common_crop.py`, `inter_panel_crop_alg.py`,
`page_number_crop_alg.py`, `rainbow_artifacts_eraser.py`), device profiles,
and the GUI kept entirely separate (`KCC_*.py`). That separation is worth
preserving independently of the language rewrite.

The question is how much of that separation should be enforced by Cargo
crate boundaries (publishable units with their own versioning) versus
plain Rust modules inside one crate.

## Decision

Two crates:

- `mangapress-core`: everything except CLI wiring — profiles, the
  per-page pipeline, crop algorithms, resize, contrast, rainbow removal,
  quantization, manga/RTL handling, archive I/O, and ebook builders
  (EPUB/CBZ/PDF). Each concern gets its own module, mirroring KCC's file
  split, but as `mod`s, not separate crates.
- `mangapress-cli`: `clap`-based argument parsing and orchestration only,
  mirroring `comic2ebook.main()`'s role.

No GUI is planned, and there is currently no second consumer of the core
logic that would justify splitting it into more independently-versioned
crates (e.g. a separate `mangapress-image` or `mangapress-epub` crate).
If that changes — a library consumer wants just the EPUB builder, say —
splitting `ebook`/`archive` out is a mechanical refactor at that point,
not a decision that needs to be front-loaded now.

## Consequences

Revisit this if/when there's a concrete second consumer of a subset of
`mangapress-core` (e.g., someone wanting just the EPUB builder as its own
crate).
