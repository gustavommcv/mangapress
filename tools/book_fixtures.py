"""Small synthetic chapter folders shared by book-output checks."""

from natsort import natsorted
from PIL import Image, ImageDraw

COMIC_INFO = b'''<ComicInfo><Series>Synthetic Series</Series><Title>Episode</Title>
<Volume>2</Volume><Number>7</Number><Writer>Zed, Ada</Writer>
<Summary>Panels &amp; ramps.</Summary></ComicInfo>'''


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
