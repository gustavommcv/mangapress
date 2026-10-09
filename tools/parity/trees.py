"""Small generated books shared by the comparison scripts: a tree of files, each page marked by its level.

`tree(names)` builds a book as a mapping of relative paths to bytes, so that a case can say exactly how a
folder or a .cbz is laid out. `write_source` puts such a mapping on disk as a folder or as a .cbz.
"""

import io
import zipfile

from PIL import Image, ImageDraw


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
