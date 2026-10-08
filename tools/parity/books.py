"""Compare small books produced by both real CLIs, not their ZIP serialization."""

import argparse
from dataclasses import dataclass
import io
import os
from pathlib import Path
import posixpath
import sys
import tempfile
from urllib.parse import unquote, urlsplit
import xml.etree.ElementTree as ET
import zipfile

from natsort import natsorted
import numpy as np
from PIL import Image, ImageDraw

from kcc_oracle import load_kcc
from parity import COLOUR_LIMIT, GRAY_LIMIT, HERE, REFERENCE_KCC, REPO, Report, run

NS = {"opf": "http://www.idpf.org/2007/opf", "dc": "http://purl.org/dc/elements/1.1/",
      "x": "http://www.w3.org/1999/xhtml", "ncx": "http://www.daisy.org/z3986/2005/ncx/",
      "ocf": "urn:oasis:names:tc:opendocument:xmlns:container"}
OPS = "{http://www.idpf.org/2007/ops}type"
IMAGE_EXTENSIONS = {".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"}
COMIC_INFO = b'''<ComicInfo><Series>Synthetic Series</Series><Title>Episode</Title>
<Volume>2</Volume><Number>7</Number><Writer>Zed, Ada</Writer>
<Summary>Panels &amp; ramps.</Summary></ComicInfo>'''


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
]


def fixture(directory, *, jacket=False, colour=False, passthrough=False):
    """Create chapter/page names whose natural and lexical orders disagree."""
    files = []
    for index, (chapter, page, shade) in enumerate(((10, 10, 160), (10, 2, 120), (2, 10, 80), (2, 2, 40))):
        extension = ("png", "webp", "gif", "jpg")[index] if passthrough else "png"
        path = directory / f"Chapter {chapter}" / f"page{page}.{extension}"
        path.parent.mkdir(parents=True, exist_ok=True)
        width = 256 if jacket and chapter == page == 2 else 128
        image = Image.new("RGB" if colour else "L", (width, 192), shade)
        draw = ImageDraw.Draw(image)
        draw.rectangle((9, 15, width // 2, 170), fill=(shade, 80, 220) if colour else 255)
        for y in range(20, 160):
            draw.line((width // 2 + 5, y, width - 8, y), fill=(y, 150, shade) if colour else y)
        image.save(path)
        files.append(path)
    (directory / "ComicInfo.xml").write_bytes(COMIC_INFO)
    return natsorted(files, key=lambda path: path.relative_to(directory).as_posix())


def member(base, reference):
    """Resolve a local EPUB URI, rejecting external or escaping references."""
    uri = urlsplit(reference)
    if uri.scheme or uri.netloc:
        raise ValueError(f"external book reference: {reference}")
    path = posixpath.normpath(posixpath.join(posixpath.dirname(base), unquote(uri.path)))
    if path.startswith(("../", "/")) or path == "..":
        raise ValueError(f"book reference escapes archive: {reference}")
    return path


def read_epub(path):
    with zipfile.ZipFile(path) as archive:
        if archive.read("mimetype") != b"application/epub+zip":
            raise ValueError("invalid EPUB mimetype")
        container = ET.fromstring(archive.read("META-INF/container.xml"))
        opf_path = container.find(".//ocf:rootfile", NS).attrib["full-path"]
        opf = ET.fromstring(archive.read(opf_path))
        items = {item.attrib["id"]: item for item in opf.findall("opf:manifest/opf:item", NS)}
        # Verify every declared resource, even if it is not in the spine.
        for item in items.values():
            archive.getinfo(member(opf_path, item.attrib["href"]))
        metadata = {key: tuple(node.text or "" for node in opf.findall(f"opf:metadata/dc:{key}", NS))
                    for key in ("title", "creator", "language", "description")}
        metadata["creators"] = tuple(sorted(metadata.pop("creator")))
        layout_keys = {"primary-writing-mode", "rendition:layout", "rendition:spread"}
        metadata["layout"] = tuple(sorted((key, node.attrib.get("content", node.text or ""))
                                          for node in opf.findall("opf:metadata/opf:meta", NS)
                                          if (key := node.attrib.get("name", node.attrib.get("property"))) in layout_keys))
        spine = opf.find("opf:spine", NS)
        direction = spine.attrib.get("page-progression-direction")
        pages, hrefs, sides = [], [], []
        for ref in spine:
            item = items[ref.attrib["idref"]]
            href = member(opf_path, item.attrib["href"])
            document = ET.fromstring(archive.read(href))
            # Kindle markup can repeat the same image in its hidden block.
            images = {member(href, node.attrib["src"]) for node in document.findall(".//x:img", NS)}
            if len(images) != 1:
                raise ValueError(f"expected one page image in {href}, got {len(images)}")
            pages.append(archive.read(images.pop()))
            hrefs.append(href)
            properties = ref.attrib.get("properties", "").split()
            sides.append(tuple(sorted(value.replace("rendition:", "") for value in properties)))
        if not pages:
            raise ValueError("empty EPUB spine")
        covers = [item for item in items.values() if "cover-image" in item.attrib.get("properties", "").split()]
        if len(covers) != 1:
            raise ValueError(f"expected one cover, got {len(covers)}")
        cover = archive.read(member(opf_path, covers[0].attrib["href"]))
        navigation = {}
        for kind in ("ncx", "nav"):
            item = next(item for item in items.values() if
                        (item.attrib["media-type"] == "application/x-dtbncx+xml" if kind == "ncx"
                         else "nav" in item.attrib.get("properties", "").split()))
            href = member(opf_path, item.attrib["href"])
            document = ET.fromstring(archive.read(href))
            if kind == "ncx":
                links = [(node.find("ncx:navLabel/ncx:text", NS).text,
                          node.find("ncx:content", NS).attrib["src"])
                         for node in document.findall("ncx:navMap/ncx:navPoint", NS)]
            else:
                toc = next(node for node in document.findall(".//x:nav", NS) if node.attrib.get(OPS) == "toc")
                links = [(node.text, node.attrib["href"]) for node in toc.findall(".//x:a", NS)]
            if not links:
                raise ValueError(f"empty {kind} navigation")
            navigation[kind] = tuple((label, hrefs.index(member(href, target))) for label, target in links)
        if navigation["ncx"] != navigation["nav"]:
            raise ValueError("NCX and EPUB3 navigation disagree")
        return {"metadata": metadata, "direction": direction, "sides": sides,
                "navigation": navigation, "pages": pages, "cover": cover}


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
    report = Report()
    for index, case in enumerate(cases):
        # Retain books for diagnosis, but use a fresh folder so stale files
        # cannot stand in for a missing CLI output on a repeat run.
        directory = Path(tempfile.mkdtemp(prefix=f"{index:02d}-", dir=args.work))
        source = directory / "Synthetic Book"
        originals = fixture(source, jacket=case.jacket, colour=case.colour,
                            passthrough=case.passthrough)
        source_bytes = {path: path.read_bytes() for path in originals + [source / "ComicInfo.xml"]}
        kcc_dir = directory / "kcc"
        kcc_dir.mkdir()
        output = kcc_dir / f"result.{case.format}"
        ours = directory / f"mangapress.{case.format}"
        png_flags = [] if case.jpeg else ["--forcepng"]
        with tempfile.TemporaryDirectory(dir=kcc_dir) as temporary:
            run(oracle + [str(kcc_dir), "--book", temporary, str(source), "-o", str(output),
                          "-f", case.format.upper(), "-p", case.profile, "-c", "0"] + png_flags + list(case.kcc))
        run([binary, str(source), "-o", str(ours), "-f", case.format, "-p", case.profile,
             "--cropping", "disabled", "--quiet"] + png_flags + list(case.ours))
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
