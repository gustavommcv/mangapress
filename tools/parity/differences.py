"""Differences from KCC 12.0.0 that mangapress has not followed yet.

Each case here is a small generated book on which the two real command-line
tools disagree today, and where mangapress is still to be changed to follow KCC
(ADR 0020 decided that it will). A case leaves this file when mangapress is
changed to do what KCC does (it then belongs in books.py). Until then this
script exits with an error, on purpose:

    python tools/parity/differences.py --kcc ../kcc-reference [--only TEXT]

It is not part of the routine comparison. `kind` says what each case is:

  "defect"  KCC's behavior is the sensible one and mangapress does not follow it;
  "quirk"   KCC's behavior is an accident of its implementation, and following
            it is a choice to make, not an obvious fix;
  "chosen"  mangapress does this on purpose, but the table does not say so yet;
  "noise"   the processed pages are the same and only their compression differs.

Every book names an author, so that the documented difference in the default
author never stands in for the difference a case is about.
"""

import argparse
from dataclasses import dataclass
import io
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib

import numpy as np
from PIL import Image, ImageDraw, JpegImagePlugin

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from epub_book import read_epub

from books import compare_book
from kcc_oracle import load_kcc
import make_corpus
from trees import INFO, MARK_BASE, MARK_STEP, comic_info, corpus_pages, encode, marked_page, png, stored, tree, write_source
from parity import HERE, REFERENCE_KCC, REPO, matches, run

FULL_TONE = ("--forcepng", "--noquantize")


def mark_of(data):
    with Image.open(io.BytesIO(data)) as image:
        histogram = image.convert("L").histogram()
    peak = max(range(20, 236), key=lambda level: sum(histogram[level - 2:level + 3]))
    return round((peak - MARK_BASE) / MARK_STEP)


def gray16(index):
    """The marked page as a 16-bit gray PNG, which Pillow reads but does not write."""
    samples = np.asarray(marked_page(index), np.uint16) * 257
    rows = b"".join(b"\x00" + row.astype(">u2").tobytes() for row in samples)
    chunk = lambda kind, data: struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    header = struct.pack(">IIBBBBB", samples.shape[1], samples.shape[0], 16, 0, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def cut_short(index):
    return png(index)[:len(png(index)) * 6 // 10]


def bookmarked_spread(directory):
    """Four source pages, the third a spread that is cut in two, each named in ComicInfo.xml's page list."""
    spread = Image.new("L", (800, 600), 255)
    spread.paste(marked_page(2), (0, 0))
    spread.paste(marked_page(3), (400, 0))
    pages = [marked_page(0), marked_page(1), spread, marked_page(4)]
    marks = '<Pages><Page Image="0" Bookmark="Start"/><Page Image="2" Bookmark="The spread"/><Page Image="3" Bookmark="After"/></Pages>'
    files = {f"Pages/{n:03d}.png": encode(page, "PNG") for n, page in enumerate(pages)}
    return dict(files, **{"ComicInfo.xml": comic_info("<Series>S</Series><Writer>Ann</Writer>" + marks).encode()}), None


def strips(directory):
    files = {f"Strip/{n:03d}.png": encode(make_corpus.strip(200 + n, 800, 2000, [(100, 600), (900, 1000)], (255, 255, 255)), "PNG") for n in (1, 2)}
    return dict(files, **{"ComicInfo.xml": INFO.encode()}), None


WRAPPED = ["My Wrapper/001.png", "My Wrapper/002.png"]


@dataclass(frozen=True)
class Difference:
    id: str
    kind: str
    name: str
    build: object
    today: str               # what happens today, for the reader of a failing run
    archive: bool = False    # give the tools a .cbz instead of a folder
    kcc: tuple = ("-c", "0") + FULL_TONE
    ours: tuple = ("--cropping", "disabled") + FULL_TONE
    profile: str = "K11"


DIFFERENCES = [
    # Page order inside a folder.
    Difference("ORD-1", "defect", "a name that another name continues comes first",
               tree(["A/p01.png", "A/p01 (2).png", "A/p01-2.png", "A/p01_b.png", "B/cover.png", "B/cover2.png", "B/x.png", "B/x1.png",
                     "C/1.png", "C/1.5.png", "C/1.10.png", "C/2.png"]),
               "mangapress puts 'p01 (2)' and 'p01-2' before 'p01', 'cover2' before 'cover', and '1.5' and '1.10' before '1'"),
    Difference("ORD-2", "defect", "full-width digits count as numbers",
               tree(["A/1.png", "A/２.png", "A/3.png", "A/１０.png", "第１話/1.png", "第２話/1.png", "第１０話/1.png"]),
               "mangapress sorts full-width digits as letters: １０ before ２, in file names and in folder names"),
    # Which files are pages, and what a damaged one does.
    Difference("FILE-3", "defect", "a PNG cut short still becomes a page, blank where the data ends",
               tree(["Pages/001.png"], extra={"Pages/002.png": cut_short}),
               "mangapress stops with an error and writes no book"),
    # Compression of the pages.
    Difference("JPEG-1", "defect", "color JPEG pages and covers keep chroma at half size in both directions",
               corpus_pages(colour=True), "mangapress keeps chroma at full size: larger files, and pixels further from KCC's than the limit allows",
               kcc=("-c", "0", "--forcecolor"), ours=("--cropping", "disabled", "--forcecolor")),
]


def attempt(command):
    done = subprocess.run(command, capture_output=True, text=True)
    lines = (done.stdout + done.stderr).strip().splitlines()
    return done.returncode, (lines[-1] if lines else "")


def describe(case, theirs, ours, legend):
    """The first way the two books differ, or None."""
    if legend:
        order = lambda book: [legend[mark] if 0 <= mark < len(legend) else "?" for mark in map(mark_of, book["pages"])]
        # A page whose mark cannot be read differs in its pixels, not in its place.
        if order(theirs) != order(ours) and "?" not in order(theirs) + order(ours):
            return f"page order: KCC {order(theirs)}, mangapress {order(ours)}"
    for label, layout in (("page", lambda book: book["pages"][-1]), ("cover", lambda book: book.get("cover"))):
        a, b = layout(theirs), layout(ours)
        if a and b and a[:2] == b[:2] == b"\xff\xd8":
            sampling = [JpegImagePlugin.get_sampling(Image.open(io.BytesIO(data))) for data in (a, b)]
            if sampling[0] != sampling[1]:
                names = {0: "full size", 1: "half width", 2: "half size both ways", -1: "none (gray)"}
                return f"{label} JPEG chroma: KCC {names[sampling[0]]} ({len(a)} bytes), mangapress {names[sampling[1]]} ({len(b)} bytes)"
    try:
        compare_book(theirs, ours, codec=True, kindle_png=True)
    except ValueError as error:
        return str(error)
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--kcc", required=True)
    parser.add_argument("--work", type=Path, default=Path(REPO) / "target/parity/differences")
    parser.add_argument("--only", action="append", help="run cases whose id or name contains this text; repeat to select more")
    args = parser.parse_args()
    checkout = str(Path(args.kcc).resolve())
    version, _, _, _ = load_kcc(checkout)
    if version != REFERENCE_KCC:
        parser.error(f"these cases describe KCC {REFERENCE_KCC}, got {version}")
    cases = [case for case in DIFFERENCES if matches(f"{case.id} {case.name}", args.only)]
    if not cases:
        parser.error("no cases match --only")
    run(["cargo", "build", "--release", "--locked", "-p", "mangapress-cli"], cwd=REPO)
    binary = str(Path(REPO) / "target/release" / ("mangapress.exe" if os.name == "nt" else "mangapress"))
    oracle = [sys.executable, str(Path(HERE) / "kcc_oracle.py"), checkout]
    args.work = args.work.resolve()
    args.work.mkdir(parents=True, exist_ok=True)
    open_cases = 0
    for case in cases:
        directory = Path(tempfile.mkdtemp(prefix=f"{case.id}-", dir=args.work))
        files, legend = case.build(directory)
        source = write_source(directory, files, case.archive)
        kcc_dir = directory / "kcc"
        kcc_dir.mkdir()
        theirs, ours = kcc_dir / "result.epub", directory / "mangapress.epub"
        with tempfile.TemporaryDirectory(dir=kcc_dir) as temporary:
            kcc_exit, kcc_said = attempt(oracle + [str(kcc_dir), "--book", temporary, str(source), "-o", str(theirs),
                                                    "-f", "EPUB", "-p", case.profile] + list(case.kcc))
        our_exit, our_said = attempt([binary, str(source), "-o", str(ours), "-f", "epub", "-p", case.profile, "--quiet"] + list(case.ours))
        if kcc_exit or our_exit:
            made = lambda code, said: "made a book" if not code else f"stopped ({said[-160:]})"
            found = None if (kcc_exit and our_exit) else f"KCC {made(kcc_exit, kcc_said)}; mangapress {made(our_exit, our_said)}"
        else:
            unreadable = (ValueError, KeyError, StopIteration, OSError, zipfile.BadZipFile)
            try:
                mine = read_epub(ours)
            except unreadable as error:
                found = f"mangapress's book could not be read: {error}"
            else:
                try:
                    found = describe(case, read_epub(theirs), mine, legend)
                except unreadable as error:
                    # The shared reader insists on one cover; a book without one is a result here, not a broken book.
                    found = "KCC's book declares no cover image, mangapress's declares one" if "expected one cover, got 0" in str(error) \
                        else f"KCC's book could not be read: {error}"
        if found:
            open_cases += 1
            print(f"DIFF {case.id} [{case.kind}] {case.name}\n       found: {found}\n       today: {case.today}", flush=True)
        else:
            print(f"same {case.id} [{case.kind}] {case.name}", flush=True)
    if open_cases:
        raise SystemExit(f"{open_cases} of {len(cases)} difference(s) from KCC {version} are still open; books are in {args.work}")
    print(f"KCC {version}: none of these {len(cases)} differences remains; move the cases into books.py")


if __name__ == "__main__":
    main()
