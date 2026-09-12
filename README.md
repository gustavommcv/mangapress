# mangapress

A CLI-first manga/comic converter for e-ink readers (Kindle, Kobo,
reMarkable) — a spiritual successor to
[KCC (Kindle Comic Converter)](https://github.com/ciromattia/kcc)'s CLI
mode, written in Rust: a single static binary, no Python runtime, no Qt
dependency.

## Status

Early scaffold. The project structure, device profile table, and CLI
argument surface exist; the image processing pipeline and ebook builders
are not implemented yet (see `todo!()` markers throughout
`crates/mangapress-core`, and `docs/adr/` for the reasoning behind each
piece). Implementation is proceeding incrementally, algorithm by algorithm,
validated against synthetic fixtures before real-world use.

## Where this fits

```
HakuNeko (downloads chapters)
    -> Mangabind (organizes into per-chapter .cbz volumes)
    -> mangapress (resizes/optimizes for e-ink, generates EPUB)
    -> KOReader / Kindle
```

mangapress reads a `.cbz` (or a folder of chapter subfolders) and writes
EPUB (priority), CBZ, or PDF, optimized for a target device's screen
resolution and e-ink display characteristics. It does not download
anything or touch the network — that's Mangabind's and HakuNeko's job.

## Why not just use KCC?

KCC solves the image-processing side well, but its CLI mode
(`kcc-c2e.py`) — while technically independent of PyQt/PySide6 at the code
level — is packaged as a single `install_requires` list with no
`extras_require` split, forcing a full PySide6/Qt install even for
CLI-only use. This is a real, reproducible packaging gap (see
`docs/adr/0002-pip-workaround-tested-and-rejected.md`), but it can be
worked around by running from source in a venv without PySide6. mangapress
exists for reasons beyond that one packaging bug: a single static binary
with no interpreter/runtime to install, and a real automated test suite —
both explicitly absent from upstream KCC today.

## Relationship to upstream KCC

KCC is used as a reference/specification, not a source to copy from
wholesale — see `docs/adr/0007-gplv3-boundary-kcc-image-rs.md` for why
`image.py` and `dualmetafix.py` specifically (GPLv3-licensed, unlike the
rest of the ISC-licensed repo) are treated as algorithm documentation to
reimplement independently, not code to port.

## License

Dual-licensed under MIT or Apache-2.0, at your option — see `LICENSE-MIT`
and `LICENSE-APACHE`.
