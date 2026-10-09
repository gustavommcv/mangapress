"""Differences from KCC 12.0.0 that nobody has decided yet.

Each case here is a small generated book on which the two real command-line
tools disagree today. None of them is in ADR 0013's table of deliberate
differences. A case stops failing when mangapress is changed to do what KCC
does, or leaves this file when the difference is accepted and written into
that table. Until then this script exits with an error, on purpose:

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
from trees import INFO, MARK_BASE, MARK_STEP, comic_info, encode, marked_page, png, stored, tree, write_source
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


def corpus_pages(colour):
    def build(directory):
        pages = [make_corpus.page(140 + n, colour=True) for n in range(3)] if colour else [make_corpus.page(100 + n) for n in range(4)]
        files = {f"Pages/{n:03d}.png": encode(page, "PNG") for n, page in enumerate(pages)}
        return dict(files, **{"ComicInfo.xml": INFO.encode()}), None
    return build


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


CUSTOM = ("--customwidth", "800", "--customheight", "1200")
CHAPTERS = ["Ch 1/001.png", "Ch 1/002.png", "Ch 2/001.png"]
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
    # ORD-3 also meets TOC-1: its loose pages are an entry of their own.
    Difference("ORD-3", "defect", "a folder's own pages come before its subfolders",
               tree(["cover.png", "zz-credits.png", "Chapter 1/001.png", "Chapter 1/002.png", "Chapter 2/001.png"]),
               "mangapress puts pages lying beside the chapter folders after the chapters whose names sort first; the cover changes with it"),
    Difference("ORD-4", "defect", "the same, one level down",
               tree(["Vol 1/intro.png", "Vol 1/zz.png", "Vol 1/Ch 1/1.png", "Vol 1/Ch 2/1.png"]),
               "as ORD-3, inside a volume folder"),
    Difference("ORD-5", "quirk", "chapter folders are ordered by their names written in plain ASCII",
               tree(["Émile/1.png", "Eric/1.png", "Frank/1.png", "漫画/1.png", "一/1.png", "_notes/1.png", "[bonus]/1.png", "#extra/1.png"]),
               "KCC orders by a transliteration of each name (Émile as emile, 漫画 as man-hua, [bonus] as bonus); mangapress by the names as written"),
    # Table of contents.
    Difference("TOC-1", "defect", "pages directly in the book are listed under the book's title",
               tree(["001.png", "002.png", "003.png"]),
               "mangapress labels that entry 'Untitled'"),
    Difference("TOC-2", "defect", "a .cbz whose pages sit in one top folder is read as if that folder were not there",
               tree(WRAPPED), "mangapress lists the folder's name where KCC lists the book's title", archive=True),
    # ComicInfo.xml.
    Difference("META-1", "defect", "ComicInfo.xml inside the single top folder of a .cbz is used",
               tree(WRAPPED, extra={"My Wrapper/ComicInfo.xml": INFO.encode()}, info=None),
               "mangapress ignores it: the title, authors and summary are missing", archive=True),
    Difference("META-2", "defect", "a ComicInfo.xml that is not well formed is ignored and the book is still made",
               tree(CHAPTERS, info="<ComicInfo><Series>S</Series><Writer>Ann</Writer>"),
               "mangapress stops with an error and writes no book"),
    Difference("META-3", "defect", "a ComicInfo.xml in UTF-16 is read",
               tree(CHAPTERS, info=('<?xml version="1.0" encoding="utf-16"?>' + comic_info("<Series>Sixteen</Series><Writer>Ann</Writer>")).encode("utf-16")),
               "mangapress stops with an error and writes no book"),
    Difference("META-4", "quirk", "one empty element makes KCC drop the whole ComicInfo.xml",
               tree(CHAPTERS, info=comic_info("<Series>S</Series><Volume>3</Volume><Writer>Ann</Writer><Summary></Summary>")),
               "KCC falls back to the folder name and its own name as author; mangapress uses the other fields"),
    Difference("META-5", "quirk", "text is taken as written: surrounding white space kept, entities decoded twice",
               tree(CHAPTERS, info=comic_info("<Series>\n  Tom &amp;amp; Jerry\n</Series><Writer> Ann ,  Bo</Writer>")),
               "KCC keeps the line breaks and spaces and gives 'Tom & Jerry'; mangapress trims and gives 'Tom &amp; Jerry'"),
    Difference("META-6", "defect", "a negative issue number is padded after its sign",
               tree(CHAPTERS, info=comic_info("<Series>S</Series><Number>-1</Number><Writer>Ann</Writer>")),
               "KCC writes '#-01', mangapress '#0-1'"),
    Difference("TOC-3", "quirk", "a bookmark on a page that is cut in two points at its last piece",
               bookmarked_spread, "KCC's entry opens the second half (or, with the turned copy kept, that copy); mangapress's opens the first half"),
    Difference("TOC-4", "quirk", "a webtoon book declares no cover",
               strips, "mangapress declares one", kcc=("-w", "--forcepng"), ours=("-w", "--forcepng")),
    # Which files are pages, and what a damaged one does.
    Difference("FILE-1", "defect", "AVIF and JPEG 2000 files are pages",
               tree(["Pages/001.png", "Pages/004.png"], extra={"Pages/002.avif": stored("AVIF", quality=90), "Pages/003.jp2": stored("JPEG2000")}),
               "mangapress leaves them out with a warning that counts them; a book of only such pages is refused"),
    Difference("FILE-2", "chosen", "BMP files are not pages",
               tree(["Pages/001.png", "Pages/003.png"], extra={"Pages/002.bmp": stored("BMP")}),
               "mangapress includes them (the coverage map says so; ADR 0013's table does not)"),
    Difference("FILE-3", "defect", "a PNG cut short still becomes a page, blank where the data ends",
               tree(["Pages/001.png"], extra={"Pages/002.png": cut_short}),
               "mangapress stops with an error and writes no book"),
    Difference("FILE-4", "quirk", "a 16-bit gray PNG comes out almost white",
               tree(["Pages/001.png"], extra={"Pages/002.png": gray16}),
               "KCC keeps only the lowest 255 of 65535 levels; mangapress scales the levels and shows the page"),
    # Compression of the pages.
    Difference("JPEG-1", "defect", "color JPEG pages and covers keep chroma at half size in both directions",
               corpus_pages(colour=True), "mangapress keeps chroma at full size: larger files, and pixels further from KCC's than the limit allows",
               kcc=("-c", "0", "--forcecolor"), ours=("--cropping", "disabled", "--forcecolor")),
    Difference("JPEG-2", "noise", "default JPEG pages of textured gray art",
               corpus_pages(colour=False), "the lossless pages are identical; at quality 85 the two encoders round differently, by more than the limit on dense texture",
               kcc=(), ours=()),
    # Device settings that a custom size changes.
    Difference("CUST-1", "defect", "with a custom size an old Kindle gets sixteen gray levels",
               corpus_pages(colour=False), "mangapress keeps the device's own four (Kindle 1) or fifteen (Kindle 2) levels",
               kcc=("-c", "0", "--forcepng") + CUSTOM, ours=("--cropping", "disabled", "--forcepng") + CUSTOM, profile="K1"),
    Difference("CUST-2", "defect", "with a custom size a Scribe or Colorsoft goes back to JPEG quality 85",
               corpus_pages(colour=False), "mangapress keeps quality 90, for pages and for the cover",
               kcc=("-c", "0") + CUSTOM, ours=("--cropping", "disabled") + CUSTOM, profile="KS3"),
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
