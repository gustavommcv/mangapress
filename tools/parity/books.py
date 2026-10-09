"""Compare small books produced by both real CLIs, not their ZIP serialization."""

import argparse
from dataclasses import dataclass
import io
import os
from pathlib import Path
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile

from natsort import natsorted
import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from book_fixtures import COMIC_INFO, fixture
from epub_book import member, read_epub

import book_inputs
from kcc_oracle import load_kcc
from parity import COLOUR_LIMIT, GRAY_LIMIT, HERE, REFERENCE_KCC, REPO, Report, matches, run
from trees import tree as book_tree, write_source

IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"}


@dataclass(frozen=True)
class Case:
    name: str
    format: str = "epub"
    profile: str = "K11"
    kcc: tuple = ()
    ours: tuple = ()
    passthrough: bool = False
    jacket: bool = False
    colour: bool = False
    jpeg: bool = False
    # A book from book_inputs.BOOKS instead of the small chapter fixture.
    pages: str = ""
    # Leave each tool's own cropping on; the small fixture's flat pages have nothing to crop.
    crop: bool = False
    # A book laid out exactly as given: a callable like trees.tree(...), written as a folder or, with `archive`, as a .cbz.
    tree: object = None
    archive: bool = False


# Lossless pages, so that what is compared is what each tool read and did, not how it compressed the result.
FULL_TONE = ("--noquantize",)
COLOUR_KEPT = FULL_TONE + ("--forcecolor", "--force-png-rgb", "--nokepub")

# What Mangabind writes into the .cbz of a volume: one folder for the volume, one for each chapter, pages numbered p0001.
MANGABIND_VOLUME = ["v001 - Vol.01/c001 - One/p0001.png", "v001 - Vol.01/c001 - One/p0002.png", "v001 - Vol.01/c002 - Two/p0001.png"]
MANGABIND_SERIES = MANGABIND_VOLUME + ["v002 - Vol.02/c003 - Three/p0001.png"]
AUTHOR = ("-a", "Ada")
CHAPTERS = ["Ch 1/001.png", "Ch 1/002.png", "Ch 2/001.png"]

CASES = [
    Case("EPUB: metadata, natural order, cover and navigation"),
    Case("EPUB: RTL, inverted direction and shifted spreads", kcc=("-m", "--invertdirection", "--spreadshift"),
         ours=("-m", "--invertdirection", "--spreadshift")),
    Case("EPUB: centered landscape pages", kcc=("--onepagelandscape",), ours=("--onepagelandscape",)),
    Case("EPUB: smart jacket cover filled to the screen", kcc=("--smartcovercrop", "--coverfill", "-m"),
         ours=("--smartcovercrop", "--coverfill", "-m"), jacket=True),
    Case("CBZ: original bytes and ComicInfo", "cbz", kcc=("--noprocessing", "--keepcomicinfo", "1"),
         ours=("--noprocessing", "--keepcomicinfo"), passthrough=True),
    Case("CBZ: quantized PNG", "cbz", "KoLC"),
    Case("CBZ: legacy grayscale PNG", "cbz", "KoLC", ("--pnglegacy",), ("--pnglegacy",)),
    Case("CBZ: full-tone PNG", "cbz", "KoLC", ("--noquantize",), ("--noquantize",)),
    Case("CBZ: profile default JPEG (85)", "cbz", "K11", jpeg=True),
    Case("EPUB: stored forms of a page, full-tone gray", kcc=FULL_TONE, ours=FULL_TONE, pages="decoded inputs"),
    Case("EPUB: page shapes, cropped as by default", kcc=FULL_TONE, ours=FULL_TONE, pages="geometry", crop=True),
    Case("EPUB: gray-or-colour decision, gray output", kcc=FULL_TONE, ours=FULL_TONE, pages="colour decision"),
    # The book Mangabound hands over: no ComicInfo.xml, the author given on the command line.
    Case("EPUB: a Mangabind volume as a .cbz", kcc=FULL_TONE + AUTHOR, ours=FULL_TONE + AUTHOR,
         tree=book_tree(MANGABIND_VOLUME, info=None), archive=True),
    Case("EPUB: pages lying directly in the book are listed under its title", kcc=FULL_TONE, ours=FULL_TONE,
         tree=book_tree(["001.png", "002.png", "003.png"])),
    # A ComicInfo.xml that cannot be read is ignored and the book is made; the author is given, as the default author differs on purpose.
    Case("EPUB: a ComicInfo.xml that is not well formed is ignored", kcc=FULL_TONE + AUTHOR, ours=FULL_TONE + AUTHOR,
         tree=book_tree(CHAPTERS, info="<ComicInfo><Series>S</Series><Writer>Ann</Writer>")),
    Case("EPUB: a ComicInfo.xml in UTF-16 is read", kcc=FULL_TONE, ours=FULL_TONE,
         tree=book_tree(CHAPTERS, info=('<?xml version="1.0" encoding="utf-16"?>'
                                        "<ComicInfo><Series>Sixteen</Series><Writer>Ann</Writer></ComicInfo>").encode("utf-16"))),
    Case("EPUB: a Mangabind series of two volumes as a .cbz", kcc=FULL_TONE + AUTHOR, ours=FULL_TONE + AUTHOR,
         tree=book_tree(MANGABIND_SERIES, info=None), archive=True),
]
EXTENDED_CASES = [
    Case("EPUB: combined ComicInfo title", kcc=("--metadatatitle", "1"), ours=("--metadatatitle", "combine")),
    Case("EPUB: ComicInfo title only", kcc=("--metadatatitle", "2"), ours=("--metadatatitle", "title-only")),
    Case("EPUB: explicit metadata overrides", kcc=("-t", "Chosen title", "-a", "Chosen author", "--language", "pt-BR"),
         ours=("-t", "Chosen title", "-a", "Chosen author", "--language", "pt-BR")),
    Case("EPUB: passthrough", kcc=("--noprocessing",), ours=("--noprocessing",), passthrough=True),
    Case("EPUB: Kobo page-side properties", profile="KoLC", kcc=("--nokepub",), ours=("--nokepub",)),
    Case("CBZ: oldest Kindle grayscale container", "cbz", "K1"),
    Case("CBZ: color PNG", "cbz", "KoLC", ("--forcecolor", "--force-png-rgb"),
         ("--forcecolor", "--force-png-rgb"), colour=True),
    Case("CBZ: Scribe default JPEG (90)", "cbz", "KS3", jpeg=True),
    Case("CBZ: Colorsoft default JPEG (90)", "cbz", "KCS", jpeg=True),
    Case("EPUB: stored forms of a page, colour kept", profile="KoC", kcc=COLOUR_KEPT, ours=COLOUR_KEPT, pages="decoded inputs"),
    Case("EPUB: page shapes, enlarged, right to left, spreads cut and turned", kcc=FULL_TONE + ("-u", "-m", "-r", "2"),
         ours=FULL_TONE + ("-u", "-m", "--splitter", "both"), pages="geometry", crop=True),
    Case("EPUB: gray-or-colour decision, colour asked for", profile="KoC", kcc=COLOUR_KEPT, ours=COLOUR_KEPT,
         pages="colour decision"),
]


def read_cbz(path):
    with zipfile.ZipFile(path) as archive:
        names = natsorted(name for name in archive.namelist() if Path(name).suffix.lower() in IMAGE_EXTENSIONS)
        if not names:
            raise ValueError("empty CBZ")
        return {"pages": [archive.read(name) for name in names],
                "comicinfo": archive.read("ComicInfo.xml") if "ComicInfo.xml" in archive.namelist() else None}


def compare_image(theirs, ours, *, codec=False, kindle_png=False):
    with Image.open(io.BytesIO(theirs)) as a, Image.open(io.BytesIO(ours)) as b:
        if a.size != b.size:
            raise ValueError(f"image size: KCC {a.size}, mangapress {b.size}")
        if codec:
            allowed_png = kindle_png and a.format == "GIF" and b.format == "PNG"
            if a.format != b.format and not allowed_png:
                raise ValueError(f"codec: KCC {a.format}, mangapress {b.format}")
            if a.format == b.format == "PNG" and theirs[24:26] != ours[24:26]:
                # IHDR: bit depth and color type, not compression bytes.
                raise ValueError(f"PNG depth/type: KCC {list(theirs[24:26])}, mangapress {list(ours[24:26])}")
            if a.format == "JPEG" and a.quantization != b.quantization:
                raise ValueError("JPEG quantization tables differ")
        x, y = np.asarray(a.convert("RGB"), dtype=np.int16), np.asarray(b.convert("RGB"), dtype=np.int16)
        gray = np.array_equal(x[:, :, 0], x[:, :, 1]) and np.array_equal(x[:, :, 1], x[:, :, 2])
        ours_gray = np.array_equal(y[:, :, 0], y[:, :, 1]) and np.array_equal(y[:, :, 1], y[:, :, 2])
        if gray != ours_gray:
            raise ValueError("grayscale/color classification differs")
        limit = GRAY_LIMIT if gray else COLOUR_LIMIT
        difference = np.abs(x - y).mean()
        if difference > limit:
            raise ValueError(f"pixels differ by {difference:.3f} levels (limit {limit})")


def compare_book(theirs, ours, *, codec=False, originals=None, kindle_png=False):
    if not theirs["pages"] or not ours["pages"]:
        raise ValueError("empty book comparison")
    if len(theirs["pages"]) != len(ours["pages"]):
        raise ValueError(f"page count: KCC {len(theirs['pages'])}, mangapress {len(ours['pages'])}")
    for key in ("metadata", "direction", "sides", "navigation", "comicinfo"):
        if theirs.get(key) != ours.get(key):
            raise ValueError(f"{key}: KCC {theirs.get(key)}, mangapress {ours.get(key)}")
    for number, (a, b) in enumerate(zip(theirs["pages"], ours["pages"]), 1):
        try:
            compare_image(a, b, codec=codec, kindle_png=kindle_png)
        except ValueError as error:
            raise ValueError(f"page {number}: {error}") from error
    if originals is not None and (theirs["pages"] != originals or ours["pages"] != originals):
        raise ValueError("passthrough did not preserve the ordered source bytes")
    if "cover" in theirs:
        compare_image(theirs["cover"], ours["cover"], codec=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kcc", required=True)
    parser.add_argument("--work", type=Path, default=Path(REPO) / "target/parity/books")
    parser.add_argument("--extended", action="store_true", help="include pre-release interactions and JPEG qualities")
    parser.add_argument("--only", action="append", help="run cases whose names contain this text; repeat to select more")
    args = parser.parse_args()
    checkout = str(Path(args.kcc).resolve())
    version, _, _, _ = load_kcc(checkout)
    if version != REFERENCE_KCC:
        parser.error(f"book comparisons require the named KCC {REFERENCE_KCC}, got {version}")
    run(["cargo", "build", "--release", "--locked", "-p", "mangapress-cli"], cwd=REPO)
    binary = str(Path(REPO) / "target/release" / ("mangapress.exe" if os.name == "nt" else "mangapress"))
    oracle = [sys.executable, str(Path(HERE) / "kcc_oracle.py"), checkout]
    args.work = args.work.resolve()
    args.work.mkdir(parents=True, exist_ok=True)
    cases = CASES + (EXTENDED_CASES if args.extended else [])
    qualities = (1, 50, 85, 90, 100) if args.extended else (85,)
    for quality in qualities:
        cases.append(Case(f"CBZ: JPEG quality {quality}", "cbz", "KoLC",
                          ("--jpeg-quality", str(quality)), ("--jpeg-quality", str(quality)), jpeg=True))
    if args.only:
        cases = [case for case in cases if matches(case.name, args.only)]
        if not cases:
            parser.error("no cases match --only")
    report = Report()
    for index, case in enumerate(cases):
        # Retain books for diagnosis, but use a fresh folder so stale files
        # cannot stand in for a missing CLI output on a repeat run.
        directory = Path(tempfile.mkdtemp(prefix=f"{index:02d}-", dir=args.work))
        source = directory / "Synthetic Book"
        if case.tree:
            files, _ = case.tree(directory)
            source = write_source(directory, files, case.archive)
            originals = []
            source_bytes = {path: path.read_bytes() for path in ([source] if source.is_file() else sorted(source.rglob("*")))
                            if path.is_file()}
        else:
            if case.pages:
                originals = book_inputs.write(source, book_inputs.BOOKS[case.pages]())
                (source / "ComicInfo.xml").write_bytes(COMIC_INFO)
            else:
                originals = fixture(source, jacket=case.jacket, colour=case.colour,
                                    passthrough=case.passthrough)
            source_bytes = {path: path.read_bytes() for path in originals + [source / "ComicInfo.xml"]}
        kcc_dir = directory / "kcc"
        kcc_dir.mkdir()
        output = kcc_dir / f"result.{case.format}"
        ours = directory / f"mangapress.{case.format}"
        png_flags = [] if case.jpeg else ["--forcepng"]
        kcc_crop, our_crop = ([], []) if case.crop else (["-c", "0"], ["--cropping", "disabled"])
        with tempfile.TemporaryDirectory(dir=kcc_dir) as temporary:
            run(oracle + [str(kcc_dir), "--book", temporary, str(source), "-o", str(output),
                          "-f", case.format.upper(), "-p", case.profile] + kcc_crop + png_flags + list(case.kcc))
        run([binary, str(source), "-o", str(ours), "-f", case.format, "-p", case.profile,
             "--quiet"] + our_crop + png_flags + list(case.ours))
        try:
            if any(path.read_bytes() != data for path, data in source_bytes.items()):
                raise ValueError("a CLI modified its source files")
            read = read_epub if case.format == "epub" else read_cbz
            compare_book(read(output), read(ours), codec=not case.passthrough,
                         kindle_png=case.format == "epub" and case.profile == "K11",
                         originals=[source_bytes[path] for path in originals] if case.passthrough else None)
        except (ValueError, KeyError, StopIteration, ET.ParseError, OSError, zipfile.BadZipFile) as error:
            report.fail(case.name, str(error))
            print(f"FAIL {case.name}: {error}", flush=True)
        else:
            report.ok()
            print(f"ok   {case.name}", flush=True)
    if report.failures:
        raise SystemExit(f"{len(report.failures)} book comparison(s) failed; inspect {args.work}")
    print(f"KCC {version}: {report.checked} CLI-to-book comparisons passed")


if __name__ == "__main__":
    main()
