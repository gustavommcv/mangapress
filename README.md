# mangapress

[![CI](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml/badge.svg)](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/gustavommcv/mangapress)](https://github.com/gustavommcv/mangapress/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

mangapress resizes and optimizes a manga/comic `.cbz` for e-ink reading, generating a fixed-layout
EPUB (or CBZ/PDF) tuned to a target device's screen resolution and grayscale palette — a CLI-first
Rust rewrite of [KCC (Kindle Comic Converter)](https://github.com/ciromattia/kcc)'s conversion
pipeline, distributed as a single static binary.

```
HakuNeko (downloads chapter by chapter)
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
[HakuNeko](https://github.com/manga-download/hakuneko)'s and
[Mangabind](https://github.com/gustavommcv/mangabind)'s job, respectively; mangapress only ever
reads a finished `.cbz`/folder and writes a converted book.

## Status

Functional: device profiles for ~40 Kindle/Kobo/reMarkable/generic targets, the full image
pipeline (margin and page-number-aware cropping, inter-panel cropping, resize, gamma/autocontrast,
double-page-spread split/rotate, rainbow-artifact removal for color e-ink), `ComicInfo.xml`
metadata resolution, and EPUB/CBZ/PDF output all work end to end and are covered by an extensive
test suite. Still pre-1.0 — see [docs/adr](docs/adr/README.md) for the design decisions made so
far, and open an issue if you hit a rough edge.

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
- `--format <epub|cbz|pdf>` — output format (default `epub`).
- `--manga-style` — right-to-left reading order and spread-split order.
- `--cropping <disabled|margins|margins-and-page-numbers>` — margin detection, with or without
  page-number-aware trimming (default: both).
- `--splitter <split|rotate|both>` — how to handle double-page spreads.
- `--eraserainbow` — attenuate Moire interference on color e-ink (Kaleido-style) panels.
- `--keepcomicinfo` — carry the source's `ComicInfo.xml` through into `.cbz` output.

Run `mangapress --help` for the full list, including cropping-aggressiveness tuning
(`--croppingpower`, `--croppingminimum`, `--preservemargin`), resize behavior (`--upscale`,
`--stretch`, `--wallpaper`, `--whiteborders`), and metadata overrides (`--title`, `--author`,
`--metadatatitle`, `--language`).

## Relationship to upstream KCC

KCC is used as a reference/specification, not a source to copy from wholesale — see
[docs/adr/0007-gplv3-boundary-kcc-image-rs.md](docs/adr/0007-gplv3-boundary-kcc-image-rs.md) for
why `image.py` and `dualmetafix.py` specifically (GPLv3-licensed, unlike the rest of the
ISC-licensed repo) are treated as algorithm documentation to reimplement independently, not code
to port.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
