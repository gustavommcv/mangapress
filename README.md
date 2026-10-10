# mangapress

[![CI](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml/badge.svg)](https://github.com/gustavommcv/mangapress/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/gustavommcv/mangapress)](https://github.com/gustavommcv/mangapress/releases/latest)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

mangapress converts manga and comics into EPUB, CBZ, or PDF books for your e-reader. It crops
margins, sizes pages for your screen, adjusts contrast, and handles double-page spreads. Choose a
device profile for Kindle, Kobo, reMarkable, or other screens, or set your own page dimensions.

It is a standalone command-line tool written in Rust, with image processing based on
[Kindle Comic Converter](https://github.com/ciromattia/kcc). Use it directly with a CBZ or image
folder, after [Mangabind](https://github.com/gustavommcv/mangabind) groups chapters into volumes,
or through the [Mangabound](https://github.com/gustavommcv/mangabound) desktop app.

[Download](https://github.com/gustavommcv/mangapress/releases/latest) ·
[Get started](#usage) · [Contribute](CONTRIBUTING.md)

## Install

Download and extract a package from [GitHub Releases](https://github.com/gustavommcv/mangapress/releases/latest),
or use the install script below. Packages are available for Windows x64, Linux x64, and macOS
on Intel and Apple Silicon. Running a release does not require Rust.

**macOS / Linux:**

```sh
curl -fsSL https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.ps1 | iex
```

The scripts install the latest release to `~/.local/bin` on macOS/Linux or
`%LOCALAPPDATA%\Programs\mangapress` on Windows. Follow the printed `PATH` instructions if needed;
on Windows, restart your terminal after installation. The Unix install folder can be changed with
`MANGAPRESS_INSTALL_DIR`; the same variable also works on Windows.

The installers resolve the release once, verify the selected package against that release's
`checksums.txt`, and check the executable's version before replacing an installation. Missing
checksums or failed verification stop installation. This checks download integrity, not signed
authenticity: the package and checksums are published by the same repository.
Archives contain the executable, license texts, and [third-party notices](THIRD-PARTY-NOTICES.md).
The Unix installer keeps the included notices in `mangapress-licenses` inside the install folder;
Windows keeps them beside the executable. Older releases without a checksum file must be installed
manually rather than bypassing verification.

To select a published version, set `MANGAPRESS_VERSION` (with or without its `v` prefix):

```sh
curl -fsSL https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.sh | MANGAPRESS_VERSION=v0.7.3 sh
```

```powershell
$env:MANGAPRESS_VERSION = 'v0.7.3'
irm https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.ps1 | iex
```

Unset the variable or use `latest` to return to the latest release. For automated Windows
installations that should not change the user `Path`, set `MANGAPRESS_NO_PATH_UPDATE=1`.

With the Rust version listed in [contributor setup](CONTRIBUTING.md#setup-and-checks),
you can also build the current development version:

```sh
cargo install --git https://github.com/gustavommcv/mangapress --locked mangapress-cli
```

### Update

Repeat the installation command you used. Check the installed version with `mangapress --version`.
For a Git-based Cargo installation, add `--force` to rebuild the current version.

### Uninstall

On macOS/Linux, delete `~/.local/bin/mangapress` and `~/.local/bin/mangapress-licenses`,
or those same entries in your custom install folder. Do not delete a shared `bin` folder.
On Windows, delete `%LOCALAPPDATA%\Programs\mangapress` and optionally remove that folder from
your user `Path` in Environment Variables. For a shared custom folder, remove only the executable
and included notices, not the folder itself. For a Cargo installation, run
`cargo uninstall mangapress-cli`.

## Usage

Convert a volume for the Kindle 11 screen:

```sh
mangapress "Volume 1.cbz" --profile K11 --format epub --output "Books/Volume 1.epub"
```

Missing output folders are created automatically. Input can be a CBZ, a folder
of images, or a folder containing chapter subfolders. Pages can be JPEG (`.jpg` or `.jpeg`),
PNG, GIF, BMP, or WebP. Other image formats are not decoded, even if renamed to a supported
extension. CBR, CB7, EPUB, and PDF input are not supported.

`--noprocessing` preserves image bytes instead of converting them. BMP pages cannot be
embedded unchanged in EPUB: remove that flag to convert them, or use CBZ to keep the originals.
The restriction is detected during processing, not by `--dry-run`.

Inside folder input, symbolic links are followed only to regular files within that folder.
External, broken, and directory links are skipped with a warning, including on `--dry-run`.
If you intentionally use external links, copy their files into the input instead.

Use `--output` for a file or directory. Without it, output is written beside the input; a Kobo
profile's EPUB uses the `.kepub.epub` extension. Add `--nokepub` for a plain `.epub`.
Derived filenames follow the source, not the book title; folders keep dots in their names.
Existing files are never replaced, even with an explicit output file: a suffix such as
` (mangapress)` is added instead, and the command reports the path used.
Choose a format supported by your reading app: a Kindle profile sets screen dimensions but
does not create MOBI or AZW3 files for the Kindle's native reader.

List device profiles, or preview the chapters, metadata, and output path before converting:

```sh
mangapress --list-profiles
mangapress "Volume 1.cbz" --profile K11 --format epub --dry-run
```

`--dry-run` inspects the book and checks the destination without creating anything. It cannot
guarantee future write permissions or free space. A real run checks write access before page
processing, then stages and synchronizes the completed book before publishing it. Run
`mangapress --help` for the full list of options and defaults.

The Kindle DX/DXG profile keeps its built-in 824 × 1000 size in `--list-profiles`. For CBZ
output (including `auto`), it targets 824 × 1200, as KCC 12.0.0 does. Setting either custom
dimension disables that special case; EPUB and PDF retain the built-in size.
Unmodified Scribe profiles cap the EPUB target width at 1920, also following KCC;
CBZ/PDF and custom dimensions retain their full resolution.

One input file or uncompressed CBZ entry is limited to 256 MiB; this is not a limit on the
whole book. Oversized images are checked before pixel decoding. See
[input limits](docs/adr/0015-bounded-input-reads.md) for the KCC thresholds and memory limitations.

### Common options

| Option | What it changes |
| --- | --- |
| `--profile CODE` | Target screen and device settings; the default is `KV` (Kindle Voyage). |
| `--format auto\|epub\|cbz\|pdf` | Output format. `auto` selects CBZ for Kindle, PDF for reMarkable and EPUB for the others, and EPUB whenever `--nested-toc` is given ([ADR 0024](docs/adr/0024-auto-format-is-cbz.md)). |
| `--manga-style` | Right-to-left reading and the order of split spreads. |
| `--cropping MODE` | `disabled`, `margins`, or `margins-and-page-numbers` (the default). |
| `--splitter MODE` | Split spreads into halves, rotate them, or keep both versions: `split` (default), `rotate`, or `both`. |
| `--upscale` | Enlarge pages smaller than the screen. Off by default. |
| `--forcecolor` | Preserve color pages; output is grayscale by default, including with a color-device profile. |
| `--jpeg-quality N` | JPEG quality from 1 to 100; defaults to 90 for Kindle Scribe/Colorsoft profiles and 85 for the others. |
| `--forcepng` | Dither to the profile's grayscale palette and store pages as PNG instead of JPEG. |
| `--webtoon` | Join each chapter's vertical strips and split them into pages between panels. |
| `--cover FILE` | Choose a cover image instead of using the first page. |
| `--title TEXT`, `--author TEXT`, `--language CODE` | Set book details. |

The help also covers contrast, crop tuning, borders, cover cropping, and spread placement.
`--stretch` fills the screen by changing the aspect ratio; `--wallpaper` crops to fill it.
`--eraserainbow` reduces interference patterns on color e-ink screens, independently of
`--forcecolor`.

### Custom screen dimensions

Use `OTHER` with both dimensions, in pixels. A named profile also accepts dimension overrides.

```sh
mangapress "Volume 1.cbz" --profile OTHER --customwidth 1072 --customheight 1448 --format cbz
```

### Covers and metadata

mangapress reads `ComicInfo.xml` for metadata and bookmarks. Use `--metadatatitle` to choose how
its title is used, or `--keepcomicinfo` to include the source metadata file in CBZ output.

Without `--cover`, a sibling `Covers` folder is checked for an image matching the input name:
`Covers/Volume 1.jpg` for `Volume 1.cbz`. If no covers match by name, covers are assigned by
their position among the books. Otherwise, the first page is used.

### Join labelled spreads

`--spreads FILE` joins pages scanned as separate halves before image processing. The JSON
lists each pair's first page index, starting at zero across the whole book:

```json
{ "spreads": [12, 40] }
```

This is the format used by KCC's Label Spreads feature. Without the flag, mangapress looks for
a file named after the input with `.json` appended, such as `Volume 1.cbz.json`.

### Combine volumes with chapter navigation

For an archive created by Mangabind's `--combine`, add `--nested-toc`. Volume folders become
parent entries and their chapters appear beneath them in the table of contents.

```sh
mangapress "Example Series.cbz" --profile K11 --format epub --nested-toc
```

Nested navigation currently requires EPUB; CBZ and PDF are rejected with this option.

## Machine-readable integration

`--json-events` writes progress, warnings, errors, and results as JSON Lines to stdout:

```sh
mangapress "Volume 1.cbz" --profile K11 --format epub --json-events
mangapress --list-profiles --json-events
mangapress --protocol-version
```

The [protocol reference](docs/machine-protocol-v1.md) defines the events and compatibility rules.
Use the handshake's `protocol_version` to check compatibility. A successful stream ends with a
`result` event; page progress alone does not mean the book has been saved.

## Terminal output and exit codes

Routine progress, warnings, and errors go to stderr. Help, version information, profile lists,
and dry-run summaries go to stdout; conversion writes the book to disk, not to stdout.
`--quiet` suppresses routine progress, but keeps warnings and errors visible.
With `--json-events`, stdout contains only JSON Lines and ordinary progress is suppressed;
stderr may still carry diagnostics when a command fails. Help and version requests remain
human-readable even if `--json-events` is also passed.

| Exit code | Meaning |
| --- | --- |
| `0` | Success, including help, version, profile listings, and dry-run plans. |
| `1` | Runtime failure: configuration, input, conversion, output, or the JSON event stream. |
| `2` | Invalid command-line arguments. |

Human profile lists and dry-run summaries stop quietly if the reader of a pipe closes early.
Other output errors remain failures. A closed JSON stream also fails: consumers must not
assume success without receiving the final `result` event.

A failed page reports its chapter, one-based position, and original image filename when
available. Pages generated from several images may not have a single original filename.

## Contributing and credits

See [CONTRIBUTING.md](CONTRIBUTING.md) for development, tests, and reporting bugs.

KCC 12.0.0 is the image-processing reference. mangapress is an independent project; the
[comparison tools](tools/parity/README.md) check selected scenarios against that release.
[ADR 0013](docs/adr/0013-follow-a-named-kcc-release.md) records the intended compatibility and
deliberate differences, and [ADR 0007](docs/adr/0007-gplv3-boundary-kcc-image-rs.md) explains the
implementation and licensing boundary.

## AI disclosure

Parts of mangapress, including code, tests, and documentation, were written with the help of AI
coding assistants. The maintainers direct, review, and test this work, but AI-assisted contributions
can contain mistakes like any other. If you find a bug or an error in the documentation, please
[open an issue](https://github.com/gustavommcv/mangapress/issues).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) contains credits and license notices for KCC
and Pillow.
Release packaging also generates `DEPENDENCY-LICENSES.txt` with the compiled dependencies'
source license texts and credits; see [notice generation](tools/licenses/README.md).
