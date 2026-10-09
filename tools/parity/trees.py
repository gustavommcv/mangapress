"""Small generated books shared by the comparison scripts: a tree of files, each page marked by its level.

`tree(names)` builds a book as a mapping of relative paths to bytes, so that a case can say exactly how a
folder or a .cbz is laid out. `write_source` puts such a mapping on disk as a folder or as a .cbz.
"""

import io
import struct
import zipfile
import zlib

import numpy as np
from PIL import Image, ImageDraw

import make_corpus


MARK_BASE, MARK_STEP = 40, 12


def marked_page(index):
    """A framed page with a flat patch whose level says which page it is, whatever order it ends up in."""
    image = Image.new("L", (400, 600), 255)
    draw = ImageDraw.Draw(image)
    draw.rectangle((40, 40, 359, 559), outline=0, width=6)
    draw.rectangle((80, 100, 319, 499), fill=MARK_BASE + MARK_STEP * index)
    return image


def encode(image, fmt, **options):
    data = io.BytesIO()
    image.save(data, fmt, **options)
    return data.getvalue()


def png(index):
    return encode(marked_page(index), "PNG")


def comic_info(body):
    return f"<ComicInfo>{body}</ComicInfo>"


INFO = comic_info("<Series>Synthetic Series</Series><Volume>2</Volume><Writer>Zed, Ada</Writer><Summary>Sum.</Summary>")


def tree(names, extra=None, info=INFO):
    """A book: one marked page per relative path (numbered as listed), other files, and ComicInfo.xml."""
    def build(directory):
        files = {name: png(index) for index, name in enumerate(names)}
        files.update({name: maker(len(names) + offset) if callable(maker) else maker
                      for offset, (name, maker) in enumerate((extra or {}).items())})
        if info is not None:
            files["ComicInfo.xml"] = info if isinstance(info, bytes) else info.encode()
        legend = list(names) + list(extra or {})
        return files, legend
    return build


def stored(fmt, **options):
    return lambda index: encode(marked_page(index), fmt, **options)


def corpus_pages(colour):
    """A book of procedurally drawn pages (`make_corpus`): three in colour or four in gray, with ComicInfo.xml."""
    def build(directory):
        pages = [make_corpus.page(140 + n, colour=True) for n in range(3)] if colour else [make_corpus.page(100 + n) for n in range(4)]
        files = {f"Pages/{n:03d}.png": encode(page, "PNG") for n, page in enumerate(pages)}
        return dict(files, **{"ComicInfo.xml": INFO.encode()}), None
    return build


def cut_short(maker, fraction=0.6):
    """A page cut off after `fraction` of its bytes, as an interrupted download leaves it."""
    def build(index):
        data = maker(index)
        return data[:int(len(data) * fraction)]
    return build


def palette_png(index):
    """A palette PNG whose first color is a visible red, so that a page left unread shows it."""
    image = marked_page(index).convert("RGB").quantize(8)
    colors = image.getpalette()
    colors[0:3] = [200, 30, 30]
    image.putpalette(colors)
    return encode(image, "PNG")


def transparent_gif(index, transparent=5):
    """A GIF with a transparent color, whose first color is a visible green.

    A drawn page with texture, not flat areas: where the data ends in the middle of a long
    run of one color, KCC's decoder and mangapress's differ in how much of the run they keep.
    """
    image = make_corpus.page(100 + index, colour=True).convert("RGB").resize((300, 400)).quantize(32)
    colors = image.getpalette()
    colors[0:3] = [30, 160, 40]
    image.putpalette(colors)
    return encode(image, "GIF", transparency=transparent)


def interlaced_png(index):
    """The marked page as an interlaced (Adam7) gray PNG, which Pillow reads but does not write."""
    samples = np.asarray(marked_page(index), np.uint8)
    chunk = lambda kind, data: struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b""
    for x0, y0, dx, dy in ((0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)):
        rows += b"".join(b"\x00" + row.tobytes() for row in samples[y0::dy, x0::dx])
    header = struct.pack(">IIBBBBB", samples.shape[1], samples.shape[0], 8, 0, 0, 0, 1)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def write_source(directory, files, archive):
    if archive:
        path = directory / "Synthetic Book.cbz"
        with zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as output:
            for name, data in files.items():
                output.writestr(name, data)
        return path
    path = directory / "Synthetic Book"
    for name, data in files.items():
        (path / name).parent.mkdir(parents=True, exist_ok=True)
        (path / name).write_bytes(data)
    return path
