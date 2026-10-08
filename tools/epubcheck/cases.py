"""Selected complete-book fixtures and assertions, independent of the CLI's plan."""

from dataclasses import dataclass
import hashlib
import io
import json
import xml.etree.ElementTree as ET
import zipfile

from natsort import natsorted
from PIL import Image, ImageChops, ImageDraw

from book_fixtures import COMIC_INFO, fixture
from epub_book import NS, read_epub


@dataclass(frozen=True)
class Case:
    name: str
    profile: str = "K11"
    flags: tuple = ()
    target: tuple = (1072, 1448)
    original_resolution: tuple | None = (1072, 1448)
    codec: str | None = "JPEG"
    quality: int = 85
    colour: bool = False
    scenario: str = "chapters"
    archive: bool = False
    kindle: bool = True
    reject_bmp: bool = False


CASES = (
    Case("kindle-jpeg"),
    Case("kindle-centered-png", flags=("--forcepng", "--onepagelandscape"), codec="PNG"),
    Case("kobo-color-png", "KoLC", ("--forcepng", "--forcecolor", "--force-png-rgb"),
         (1264, 1680), None, "PNG", colour=True, kindle=False),
    Case("custom-rotated-png", "OTHER", ("--forcepng", "--customwidth", "127", "--customheight", "193",
                                         "--splitter", "rotate"), (127, 193), None, "PNG", scenario="rotated", kindle=False),
    Case("kindle-four-tone", "K1", ("--forcepng", "--stretch"), (600, 670), (600, 670), "PNG",
         scenario="single"),
    Case("kindle-fifteen-tone", "K2", ("--forcepng", "--stretch"), (600, 670), (600, 670), "PNG",
         scenario="single"),
    Case("kindle-dx-bmp-input", "KDX", ("--forcepng", "--stretch"), (824, 1000), (824, 1000), "PNG",
         scenario="bmp"),
    Case("scribe-capped", "KS3", ("--forcepng", "--stretch"), (1920, 2648), (1920, 2648), "PNG",
         scenario="single"),
    Case("scribe-color-capped", "KSCS", ("--forcepng", "--stretch"), (1920, 2648), (1920, 2648), "PNG",
         scenario="single"),
    Case("scribe-custom-uncapped", "KS3", ("--forcepng", "--stretch", "--customwidth", "1986"),
         (1986, 2648), None, "PNG", scenario="single"),
    Case("remarkable-full-size", "RmkPP", ("--forcepng", "--stretch"), (1620, 2160), None, "PNG",
         scenario="single", kindle=False),
    Case("custom-even-size", "OTHER", ("--forcepng", "--stretch", "--customwidth", "128",
                                      "--customheight", "192"), (128, 192), None, "PNG", scenario="single", kindle=False),
    Case("colorsoft-default-jpeg", "KCS", ("--forcecolor", "--stretch"), (1272, 1696), (1272, 1696),
         quality=90, colour=True, scenario="single"),
    Case("nested-volumes-cbz", flags=("--nested-toc", "--forcepng", "--noquantize", "--noautocontrast",
                                    "--gamma", "1"), codec="PNG", scenario="nested", archive=True),
    Case("bookmarks-after-rtl-split", flags=("--splitter", "split", "-m"), scenario="bookmarks"),
    Case("unicode-collection", "KoLC", ("--metadatatitle", "combine", "--language", "pt-BR"),
         (1264, 1680), None, scenario="metadata", kindle=False),
    Case("explicit-metadata-and-direction", "KoLC", ("--title", 'Chosen <book> & "title"',
         "--author", "Chosen & Writer", "--language", "ja-JP", "-m", "--invertdirection", "--spreadshift"),
         (1264, 1680), None, scenario="overrides", kindle=False),
    Case("mixed-codec-passthrough-cbz", flags=("--noprocessing",), codec=None, scenario="passthrough",
         archive=True),
    Case("external-color-cover", flags=("--forcecolor",), colour=True, scenario="cover"),
    Case("bmp-passthrough-folder-refusal", flags=("--noprocessing",), scenario="bmp", reject_bmp=True),
    Case("bmp-passthrough-cbz-refusal", flags=("--noprocessing",), scenario="bmp", archive=True, reject_bmp=True),
)


def expect(actual, expected, context):
    if actual != expected:
        raise ValueError(f"{context}: expected {expected!r}, got {actual!r}")


def snapshot(paths):
    return {path: hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def rich_metadata():
    root = ET.Element("ComicInfo")
    for name, value in {"Series": "Série & 雨 <Archive>", "Title": 'Épisode <One> & "Two"',
                        "Summary": 'Panels <frames> & "notes".', "Writer": "Zoë & Z, Ada <A>, Ada <A>",
                        "Volume": "2", "Number": "7"}.items():
        ET.SubElement(root, name).text = value
    return root


def prepare_case(directory, case):
    """Return the input, extra options, ordered source bytes and immutable inputs."""
    source = directory / "Synthetic Book"
    if case.scenario in {"single", "bmp", "nested"}:
        chapters = ("Volume 10/Chapter 1", "Volume 2/Chapter 10", "Volume 2/Chapter 1") \
            if case.scenario == "nested" else ("Chapter 2",)
        paths = []
        for chapter_index, chapter in enumerate(chapters):
            for page in ((10, 2) if case.scenario == "nested" else (2,)):
                path = source / chapter / f"page{page}.{'bmp' if case.scenario == 'bmp' else 'png'}"
                path.parent.mkdir(parents=True, exist_ok=True)
                shade = 40 + (chapter_index * 2 + (page == 10)) * 30
                image = Image.new("RGB" if case.colour else "L", (32, 48), shade)
                ImageDraw.Draw(image).rectangle((4, 8, 20, 40), fill=(220, 80, 140) if case.colour else 210)
                image.save(path)
                paths.append(path)
        paths = natsorted(paths, key=lambda path: path.relative_to(source).as_posix())
        (source / "ComicInfo.xml").write_bytes(COMIC_INFO)
    else:
        paths = fixture(source, colour=case.colour, jacket=case.scenario in {"rotated", "bookmarks"},
                        passthrough=case.scenario == "passthrough")
    if case.scenario in {"metadata", "overrides"}:
        (source / "ComicInfo.xml").write_bytes(ET.tostring(rich_metadata(), encoding="utf-8"))
    elif case.scenario == "bookmarks":
        root = ET.fromstring(COMIC_INFO)
        pages = ET.SubElement(root, "Pages")
        for index, label in ((0, "Spread & start"), (1, "After spread <2>"), (3, "End")):
            ET.SubElement(pages, "Page", Image=str(index), Bookmark=label)
        (source / "ComicInfo.xml").write_bytes(ET.tostring(root, encoding="utf-8"))
    elif case.scenario == "passthrough":
        # Keep alpha in two core EPUB codecs; no processing may flatten or re-encode it.
        for path in paths:
            if path.suffix in {".png", ".webp"}:
                image = Image.new("RGBA", (128, 192), (40, 80, 160, 0))
                ImageDraw.Draw(image).rectangle((9, 15, 90, 170), fill=(220, 80, 140, 255))
                image.save(path)
    originals = tuple(path.read_bytes() for path in paths)
    immutable = list(source.rglob("*"))
    immutable = [path for path in immutable if path.is_file()]
    flags = ()
    if case.scenario == "cover":
        cover = directory / "external.jpg"
        Image.new("RGB", (128, 192), (220, 20, 40)).save(cover)
        immutable.append(cover)
        flags = ("--cover", str(cover))
    if case.archive:
        archive = directory / "input.cbz"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
            for path in reversed(sorted(source.rglob("*"))):
                if path.is_file():
                    package.write(path, path.relative_to(source).as_posix())
        immutable.append(archive)
        source = archive
    return source, flags, originals, snapshot(immutable)


def expected_toc(case):
    if case.scenario in {"single", "bmp"}:
        return (("Chapter 2", 0, ()),)
    if case.scenario == "nested":
        return (("Volume 2", 0, (("Chapter 1", 0, ()), ("Chapter 10", 2, ()))),
                ("Volume 10", 4, (("Chapter 1", 4, ()),)))
    if case.scenario == "bookmarks":
        return (("Spread & start", 0, ()), ("After spread <2>", 2, ()), ("End", 4, ()))
    return (("Chapter 2", 0, ()), ("Chapter 10", 2, ()))


def page_counts(case):
    source = 1 if case.scenario in {"single", "bmp"} else 6 if case.scenario == "nested" else 4
    return source, 5 if case.scenario == "bookmarks" else source


def read_events(stdout):
    events = [json.loads(line) for line in stdout.splitlines() if line.strip()]
    if not events or any(not isinstance(event, dict) for event in events):
        raise ValueError("missing or invalid CLI event stream")
    for sequence, event in enumerate(events, 1):
        expect((event.get("tool"), event.get("protocol_version"), event.get("sequence")),
               ("mangapress", 1, sequence), "CLI framing")
    return events


def verify_bmp_rejection(status, stdout, stderr, book):
    expect(status, 1, "BMP passthrough refusal status")
    events = read_events(stdout)
    error = events[-1]
    expect((error.get("type"), error.get("code"), error.get("stage"), error.get("severity")),
           ("error", "page_processing_failed", "process", "error"), "BMP passthrough refusal")
    explanation = "BMP pages cannot be embedded in EPUB without processing"
    recovery = "Remove --noprocessing or choose CBZ"
    if any(event.get("type") == "result" for event in events) or book.exists():
        raise ValueError("BMP passthrough refusal produced a result or book")
    if not all(text in error.get("diagnostic", "") and text in stderr for text in (explanation, recovery)):
        raise ValueError("BMP passthrough refusal lacks its specific explanation and recovery")


def verify_events(stdout, case, book, dry_run):
    events = read_events(stdout)
    plans = [event for event in events if event.get("type") == "stage" and event.get("stage") == "plan"
             and event.get("state") == "completed"]
    expect(len(plans), 1, "completed CLI plans")
    result = events[-1]
    expect((result.get("type"), result.get("status"), result.get("dry_run"), result.get("written")),
           ("result", "completed", dry_run, not dry_run), "CLI result")
    for event in (plans[0], result):
        expect((event.get("format"), event.get("profile"), event.get("width"), event.get("height")),
               ("epub", case.profile, *case.target), "effective processing target")
        expect(event.get("output_path"), str(book.resolve()), "CLI output path")
    source_pages, output_pages = page_counts(case)
    expect(result.get("source_pages"), source_pages, "source page count")
    if not dry_run:
        expect(result.get("output_pages"), output_pages, "output page count")
        expect(result.get("bytes"), book.stat().st_size, "published byte count")


def verify_book(path, case, originals):
    """Check intended book content in addition to EPUBCheck's format rules."""
    book = read_epub(path)
    expect(len(book["pages"]), page_counts(case)[1], "spine page count")
    expect(len(book["documents"]), len(book["pages"]), "page document count")
    expect(book["toc"], expected_toc(case), "TOC hierarchy and real spine targets")
    rtl = "-m" in case.flags and "--invertdirection" not in case.flags
    expect(book["direction"], "rtl" if rtl else "ltr", "reading direction")
    if "--onepagelandscape" in case.flags:
        sides = ["center"] * len(book["pages"])
    elif case.scenario == "rotated":
        sides = ["center", "left", "right", "left"]
    elif case.scenario == "overrides":
        sides = ["right", "left", "right", "left"]
    else:
        start = ("right", "left") if rtl else ("left", "right")
        sides = [start[index % 2] for index in range(len(book["pages"]))]
    expect(book["sides"], [(f"page-spread-{side}",) for side in sides], "spine placement")
    metadata = book["metadata"]
    rich = case.scenario in {"metadata", "overrides"}
    title = 'Série & 雨 <Archive> Vol. 02 #007: Épisode <One> & "Two"' if rich else "Synthetic Series Vol. 02 #007"
    creators = ("Ada <A>", "Zoë & Z") if rich else ("Ada", "Zed")
    language = "pt-BR" if rich else "en-US"
    if case.scenario == "overrides":
        title, creators, language = 'Chosen <book> & "title"', ("Chosen & Writer",), "ja-JP"
    for key, expected in {"title": (title,), "creators": creators, "language": (language,),
                          "description": ('Panels <frames> & "notes".',) if rich else ("Panels & ramps.",)}.items():
        expect(metadata[key], expected, key)
    metas = book["package"].findall("opf:metadata/opf:meta", NS)
    values = {node.attrib.get("name", node.attrib.get("property")): node.attrib.get("content", node.text)
              for node in metas}
    resolution = "x".join(map(str, case.original_resolution)) if case.original_resolution else None
    expect(values.get("original-resolution"), resolution, "Kindle original-resolution")
    expect(values.get("rendition:layout"), "pre-paginated", "fixed layout")
    expect(values.get("rendition:spread"), "landscape", "spread layout")
    expect(values.get("primary-writing-mode"), ("horizontal-rl" if rtl else "horizontal-lr")
           if case.original_resolution else None, "Kindle writing mode")
    collection = [node for node in metas if node.attrib.get("property") == "belongs-to-collection"]
    if case.kindle:
        expect(collection, [], "Kindle omits series collection")
    else:
        expect(len(collection), 1, "series collection")
        expect(collection[0].text, "Série & 雨 <Archive>" if rich else "Synthetic Series", "collection name")
        refines = "#" + collection[0].attrib["id"]
        refined = {node.attrib.get("property"): node.text for node in metas if node.attrib.get("refines") == refines}
        expect(refined.get("collection-type"), "series", "collection refinement")
        expect(refined.get("group-position"), None if case.scenario == "overrides" else "2.7", "series position")
    if case.scenario == "passthrough":
        expect(tuple(book["pages"]), originals, "naturally ordered unchanged source bytes")
    for index, (payload, document) in enumerate(zip(book["pages"], book["documents"])):
        with Image.open(io.BytesIO(payload)) as image:
            image.load()
            if case.codec:
                expect(image.format, case.codec, f"page {index} codec")
            if case.scenario == "nested":
                with Image.open(io.BytesIO(originals[index])) as original:
                    expect(image.size, original.size, f"page {index} unchanged small-page size")
                    expect(image.convert("RGB").tobytes(), original.convert("RGB").tobytes(),
                           f"page {index} naturally ordered nested pixels")
            if "--stretch" in case.flags:
                expect(image.size, case.target, f"page {index} full-size image")
            viewport = document.find("x:head/x:meta[@name='viewport']", NS)
            expect(viewport.attrib["content"], f"width={image.width}, height={image.height}", f"page {index} viewport")
            for node in document.findall(".//x:img", NS):
                expect((node.attrib.get("width"), node.attrib.get("height")), tuple(map(str, image.size)),
                       f"page {index} image dimensions")
            if case.codec == "JPEG":
                reference = io.BytesIO()
                Image.new("RGB" if case.colour else "L", (1, 1)).save(reference, format="JPEG", quality=case.quality)
                with Image.open(reference) as expected:
                    expect(image.quantization, expected.quantization, f"page {index} JPEG quality")
            if case.profile in {"K1", "K2"}:
                shades = set(image.convert("L").tobytes())
                if len(shades) > (4 if case.profile == "K1" else 15):
                    raise ValueError(f"page {index} exceeds the profile's grayscale palette")
            if case.colour:
                red, green, blue = image.convert("RGB").split()
                if not (ImageChops.difference(red, green).getbbox() or ImageChops.difference(red, blue).getbbox()):
                    raise ValueError(f"page {index} lost its synthetic colors")
    with Image.open(io.BytesIO(book["cover"])) as cover:
        expect(cover.format, "JPEG", "cover codec")
        if case.scenario == "bookmarks":
            expect(cover.size, (256, 192), "automatic cover uses the unsplit source")
        if case.scenario == "cover":
            expect(cover.size, (128, 192), "external cover size")
            red, green, blue = cover.convert("RGB").getpixel((64, 96))
            if red <= max(green, blue) + 100:
                raise ValueError("external red cover was not selected")
