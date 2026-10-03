# mangapress

[![CI](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml/badge.svg)](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/gustavommcv/mangapress)](https://github.com/gustavommcv/mangapress/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

mangapress resizes and optimizes a manga/comic `.cbz` for e-ink reading, generating a fixed-layout
EPUB (or CBZ/PDF) tuned to a target device's screen resolution and grayscale palette — a CLI-first
Rust rewrite of [KCC (Kindle Comic Converter)](https://github.com/ciromattia/kcc)'s conversion
pipeline, distributed as a single static binary.

```
Raw chapters (folder or .cbz per chapter)
    -> Mangabind (groups chapters into per-volume .cbz)
    -> mangapress (crops/resizes/optimizes for e-ink, writes EPUB/CBZ/PDF)
    -> KOReader / Kindle / Kobo / reMarkable
```

## The problem

KCC already solves the image-processing side well — cropping, resizing, contrast correction,
double-page-spread splitting, all tuned per device. What it doesn't solve is how it's shipped: its
CLI mode (`kcc-c2e.py`) is packaged as a single `install_requires` list with no CLI-only split, so
even headless/CLI use pulls in a full PySide6/Qt install. That's a real, reproducible packaging gap
(see [docs/adr/0002-pip-workaround-tested-and-rejected.md](docs/adr/0002-pip-workaround-tested-and-rejected.md))
— workable with a venv that skips PySide6, but still a Python runtime plus its dependency tree to
install and keep working across OS/distro updates. It also has no automated test suite validating
that any of its image-processing math still behaves the same after a change.

mangapress reimplements that pipeline in Rust: a single static binary with no interpreter or Qt
runtime to install, and every algorithm — cropping, resizing, contrast, spread detection, rainbow-
artifact removal — covered by an automated test suite, validated against a real Mangabind-produced
volume and cross-checked against KCC's own output. See
[docs/adr/0007-gplv3-boundary-kcc-image-rs.md](docs/adr/0007-gplv3-boundary-kcc-image-rs.md) for
how upstream KCC is used as a specification to reimplement independently, not code that gets
copied.

**Out of scope:** MOBI/AZW3 output. mangapress's target reader is
[KOReader](https://github.com/koreader/koreader), not a Kindle's native firmware, and KOReader
already reads fixed-layout EPUB well — see
[docs/adr/0008-mobi-azw3-permanently-out-of-scope.md](docs/adr/0008-mobi-azw3-permanently-out-of-scope.md).
mangapress also doesn't download or organize chapters — that's
[Mangabind](https://github.com/gustavommcv/mangabind)'s job; mangapress only ever
reads a finished `.cbz`/folder and writes a converted book.

**Color output is opt-in.** By default every page comes out grayscale, as upstream's does;
`--forcecolor` keeps color pages (and the cover) in color, on any profile, while pages with no real
color are still converted. Unlike upstream's window, the command line does not turn it on by itself
for a color-capable profile (`KCS` Kindle Colorsoft, `KoCC` Kobo Clara Colour, `KoLC` Kobo Libra
Colour): pass the flag. `--eraserainbow` (below) is a separate thing — it fixes a display artifact
of color e-ink screens, on grayscale and color pages alike.

## Status

Functional: device profiles for ~40 Kindle/Kobo/reMarkable/generic targets, the full image
pipeline (per-page background and color detection, margin and page-number-aware cropping,
inter-panel cropping, resize, gamma/autocontrast, double-page-spread split/rotate, rainbow-artifact
removal, grayscale or color output, palette quantization), webtoon mode (long strips cut into pages
between panels), `ComicInfo.xml` metadata and bookmarks, and EPUB/CBZ/PDF output all work end to end
and are covered by an extensive test suite, checked page by page against upstream KCC 12.0.0 by
running KCC's own code next to it ([tools/parity](tools/parity/README.md)) — see
[docs/adr](docs/adr/README.md) for the design decisions made so far, and open an issue if you hit
a rough edge. Still pre-1.0.

## Performance

Page processing is parallelized across every CPU core (via [`rayon`](https://github.com/rayon-rs/rayon)),
matching how upstream KCC fans work out across a `multiprocessing.Pool()` — mangapress just does it
compiled instead of interpreted. Measured converting a real 182-page, 7-chapter volume
(`--profile KV`, EPUB output) on a 6-core/12-thread AMD Ryzen 5 5600X, averaged over 3 runs each:

| | mangapress v0.4.0 | KCC 11.2.0 |
|---|---|---|
| Wall time | **~2.8s** | ~5.0s |
| Output | 186 pages, EPUB with a declared cover | 186 pages + a separate `cover.jpg`, same page content |

Both tools crop/split/resize every page identically for this volume — verified by instrumenting a
real KCC checkout to dump its own actual per-page decisions and diffing them against mangapress's,
not just by comparing final file sizes. The extra file in KCC's output is a duplicate cover image;
mangapress declares its EPUB cover by tagging the first page itself (`properties="cover-image"` in
the manifest, plus the older `<meta name="cover">` convention), rather than writing a second copy of
it the way KCC's own `cover.jpg` does.

## Install

**macOS/Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.ps1 | iex
```

Both scripts download the right binary for your OS/architecture from the
[latest release](https://github.com/gustavommcv/mangapress/releases/latest) and put it on your
PATH — no need to install Rust. Prebuilt binaries and checksums for every release are also
available there directly, if you'd rather install manually.

Already have Rust and want the dev version instead:

```bash
cargo install --git https://github.com/gustavommcv/mangapress mangapress-cli
```

### Update

Run the same install command again — it always fetches the latest release and overwrites the
existing binary in place. `mangapress --version` tells you what you currently have installed.

### Uninstall

mangapress is a single self-contained binary; there's no installer state to clean up beyond it.

- **macOS/Linux:** `rm ~/.local/bin/mangapress`
- **Windows:** delete `%LOCALAPPDATA%\Programs\mangapress\mangapress.exe`. The installer added
  that folder to your user `PATH`; if you'd rather remove that entry too, it's under Settings >
  System > About > Advanced system settings > Environment Variables > `Path` (User variables).
- **`cargo install`:** `cargo uninstall mangapress-cli`.

## Usage

```bash
mangapress /path/to/volume.cbz --profile KV --format epub
```

The input can be a `.cbz` file or a folder of chapter subfolders (the layout
[Mangabind](https://github.com/gustavommcv/mangabind) produces — see
[docs/adr/0005-mangabind-contract.md](docs/adr/0005-mangabind-contract.md)). If `--output` isn't
given, the result is written next to the input with the right extension for `--format`.

A few of the more commonly used flags:

- `--profile <CODE>` — target device (e.g. `KV` Kindle Voyage, `KPW5` Kindle Paperwhite 5,
  `KoAO` Kobo Aura ONE, `Rmk2` reMarkable 2, `OTHER` for `--customwidth`/`--customheight`).
- `--format <auto|epub|cbz|pdf>` — output format (default `auto`: CBZ for the four oldest Kindles,
  PDF for reMarkable, EPUB for everything else).
- `--manga-style` — right-to-left reading order and spread-split order.
- `--cropping <disabled|margins|margins-and-page-numbers>` — margin detection, with or without
  page-number-aware trimming (default: both).
- `--splitter <split|rotate|both>` — how to handle double-page spreads.
- `--eraserainbow` — attenuate Moire interference between halftone screentone and a color e-ink
  (Kaleido-style) panel's diagonal subpixel grid. A display-artifact fix, not color output —
  everything still converts to grayscale regardless of this flag or `--profile` (see "Not yet in
  scope" above).
- `--keepcomicinfo` — carry the source's `ComicInfo.xml` through into `.cbz` output.
- `--nested-toc` — build a two-level table of contents (a volume entry, its chapters nested
  underneath) instead of the usual flat, one-entry-per-chapter list. For an input whose chapter
  folders are themselves nested one level under a volume folder — what
  [Mangabind](https://github.com/gustavommcv/mangabind)'s `-combine` mode produces. EPUB output
  only for now; combined with `--format cbz` or `--format pdf` it's refused with a clear error. See
  [docs/adr/0012-nested-toc-for-combined-volumes.md](docs/adr/0012-nested-toc-for-combined-volumes.md).

Run `mangapress --help` for the full list, including cropping-aggressiveness tuning
(`--croppingpower`, `--croppingminimum`, `--preservemargin`), resize behavior (`--upscale`,
`--stretch`, `--wallpaper`, `--whiteborders`), and metadata overrides (`--title`, `--author`,
`--metadatatitle`, `--language`).

### Machine-readable integration

`--json-events` emits a versioned JSON Lines stream containing stage, chapter, source-page,
warning, error, and result events. It is an additional mode; ordinary terminal output is unchanged
when the flag is absent:

```bash
mangapress volume.cbz --profile KV --output volume.epub --json-events
mangapress volume.cbz --profile KV --output volume.epub --dry-run --json-events
```

Device profiles are available as structured events with `--list-profiles --json-events`.
`mangapress --protocol-version` returns the compatibility handshake used by GUI consumers. See the
[machine protocol v1 specification](docs/machine-protocol-v1.md) and
[ADR 0011](docs/adr/0011-versioned-json-lines-events.md). Consumers must check `protocol_version`
rather than infer compatibility from the release version.

## Relationship to upstream KCC

KCC is used as a reference/specification, not a source to copy from wholesale — see
[docs/adr/0007-gplv3-boundary-kcc-image-rs.md](docs/adr/0007-gplv3-boundary-kcc-image-rs.md) for
why `image.py` and `dualmetafix.py` specifically (GPLv3-licensed, unlike the rest of the
ISC-licensed repo) are treated as algorithm documentation to reimplement independently, not code
to port.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
