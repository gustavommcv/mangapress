"""Runs upstream KCC's own code over a list of images and records what it does.

This is a driver, not a port: it imports `kindlecomicconverter` from a KCC
checkout you provide and calls the same classes, in the same order, that
KCC's `imgFileProcessing()` calls, stopping short of the step that writes
the page to disk (which also deletes the source image). Nothing of KCC's is
copied into this repository.

    kcc_oracle.py <kcc_checkout> <out_dir> <list_file> [kcc-c2e arguments...]
    kcc_oracle.py <kcc_checkout> <out_dir> --webtoon-dir <directory> <width> <height>
    kcc_oracle.py <kcc_checkout> <out_dir> --book <isolated_temp_dir> [kcc-c2e arguments...]

The first line of the list is the book's first page. Output: one PNG per
page produced, and `kcc.json` describing them.
Book mode calls KCC's real CLI entry point on generated folders.
"""

import inspect
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import types
from argparse import Namespace


def unavailable_codec(*args, **kwargs):
    raise RuntimeError("the parity oracle does not exercise MozJPEG; install its real dependency to use it")


def load_kcc(checkout):
    # Sorting and filename sanitization use the real development dependencies,
    # including in book comparisons. Only unused PDF/MozJPEG imports get stubs.
    for name, attributes in {
        "pymupdf": {},
        "mozjpeg_lossless_optimization": {"optimize": unavailable_codec},
    }.items():
        try:
            __import__(name)
        except ImportError:
            module = types.ModuleType(name)
            module.__dict__.update(attributes)
            sys.modules[name] = module
    sys.path.insert(0, checkout)
    import kindlecomicconverter
    from kindlecomicconverter import comic2ebook, comic2panel, image

    return kindlecomicconverter.__version__, comic2ebook, comic2panel, image


def run_book(checkout, out_dir, temporary, arguments):
    # KCC removes KCC-* directories from its temporary root. Never let a
    # comparison reach the user's system temporary directory.
    temporary = Path(temporary).resolve()
    if temporary.parent != Path(out_dir).resolve() or any(temporary.iterdir()):
        raise SystemExit("book comparison requires a fresh temporary directory inside its output")
    for variable in ("TMPDIR", "TEMP", "TMP"):
        os.environ[variable] = str(temporary)
    tempfile.tempdir = str(temporary)
    # Windows spawn workers import this driver before unpickling KCC tasks.
    os.environ["MANGAPRESS_PARITY_KCC"] = os.path.abspath(checkout)
    _, comic2ebook, _, _ = load_kcc(checkout)
    raise SystemExit(comic2ebook.main(arguments))


def run_pages(checkout, out_dir, list_file, arguments):
    version, comic2ebook, _, image = load_kcc(checkout)
    options = comic2ebook.checkOptions(comic2ebook.makeParser().parse_args(arguments + ["unused-input"]))
    os.makedirs(out_dir, exist_ok=True)
    # 12.0.0 crops in the parser, before a spread is split; earlier releases
    # crop each piece afterwards.
    crops_first = "is_first_page" in inspect.signature(image.ComicPageParser.__init__).parameters

    pages = []
    with open(list_file, encoding="utf-8") as handle:
        paths = [line.strip() for line in handle if line.strip()]
    for number, path in enumerate(paths):
        source = (os.path.dirname(path), os.path.basename(path))
        parser = image.ComicPageParser(source, number == 0, options) if crops_first else image.ComicPageParser(source, options)
        pieces = []
        for payload in parser.payload:
            page = image.ComicPage(options, *payload)
            if not crops_first:
                if options.cropping == 2 and not options.webtoon:
                    page.cropPageNumber(options.croppingp, options.croppingm)
                if options.cropping == 1 and not options.webtoon:
                    page.cropMargin(options.croppingp, options.croppingm)
                if options.interpanelcrop > 0:
                    page.cropInterPanelEmptySections("horizontal" if options.interpanelcrop == 1 else "both")
            page.gammaCorrectImage()
            if not page.colorOutput:
                page.convertToGrayscale()
            page.autocontrastImage()
            page.resizeImage()
            page.optimizeForDisplay(options.eraserainbow, page.colorOutput)
            if not page.colorOutput and options.forcepng and not options.noquantize:
                page.quantizeImage()
            final = page.image.convert("RGB") if page.colorOutput else page.image.convert("L")
            pieces.append({"suffix": page.targetPathOrder, "mode": payload[0], "color": bool(page.color),
                           "black_background": page.fill != "white", "size": list(final.size), "image": final})
        # The order KCC's file names give the pieces of one source page.
        pieces.sort(key=lambda piece: piece["suffix"])
        for index, piece in enumerate(pieces):
            piece["file"] = f"{number:04d}_{index}.png"
            piece.pop("image").save(os.path.join(out_dir, piece["file"]))
        pages.append({"background": parser.page_background_color, "pieces": pieces})
    with open(os.path.join(out_dir, "kcc.json"), "w") as handle:
        json.dump({"version": version, "pages": pages}, handle, indent=1)
    print(f"KCC {version}: {len(pages)} source pages, {sum(len(page['pieces']) for page in pages)} output pages")


def run_webtoon(checkout, out_dir, directory, width, height):
    version, _, comic2panel, _ = load_kcc(checkout)
    comic2panel.GUI = None
    # Both steps work in place and delete what they read, so they get a copy.
    if os.path.isdir(out_dir):
        shutil.rmtree(out_dir)
    shutil.copytree(directory, out_dir)
    error = comic2panel.mergeDirectory([out_dir])
    if error:
        raise SystemExit(f"KCC mergeDirectory failed: {error}")
    (merged,) = sorted(os.listdir(out_dir))
    shutil.copy(os.path.join(out_dir, merged), os.path.join(os.path.dirname(out_dir), os.path.basename(out_dir) + "_strip.png"))
    error = comic2panel.splitImage([out_dir, merged, Namespace(height=height, width=width, debug=False)])
    if error:
        raise SystemExit(f"KCC splitImage failed: {error}")
    print(f"KCC {version}: {len(os.listdir(out_dir))} webtoon pages")


if os.environ.get("MANGAPRESS_PARITY_KCC"):
    load_kcc(os.environ["MANGAPRESS_PARITY_KCC"])


if __name__ == "__main__":
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    if sys.argv[3] == "--book":
        run_book(sys.argv[1], sys.argv[2], sys.argv[4], sys.argv[5:])
    elif sys.argv[3] == "--webtoon-dir":
        run_webtoon(sys.argv[1], sys.argv[2], sys.argv[4], int(sys.argv[5]), int(sys.argv[6]))
    else:
        run_pages(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:])
