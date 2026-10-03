"""Procedurally drawn stand-ins for comic pages.

Nothing here is, or is derived from, a real comic: every page is rectangles,
ramps, hatching and blobs laid out from a seeded random generator, so the
corpus can live in a public repository and is the same on every machine.
What matters is that the pages have the features the conversion pipeline
reacts to: margins, a page number, panels with gutters between them, light
and dark backgrounds, colour, double-page spreads, and vertical strips.
"""

import os

import numpy as np
from PIL import Image, ImageDraw, ImageOps

PAGE = (900, 1350)


def _fill_panel(draw, rng, box, tint):
    """One panel: a border, a texture, and sometimes a bubble with text lines."""
    x0, y0, x1, y1 = box
    shade = lambda v: tuple(int(v * t) for t in tint)
    draw.rectangle(box, fill=shade(235), outline=(0, 0, 0), width=4)
    kind = rng.integers(0, 4)
    if kind == 0:  # vertical ramp
        for y in range(y0 + 4, y1 - 4):
            v = 70 + 150 * (y - y0) / max(1, y1 - y0)
            draw.line([(x0 + 4, y), (x1 - 4, y)], fill=shade(v))
    elif kind == 1:  # hatching
        step = int(rng.integers(7, 15))
        for k in range(x0 - (y1 - y0), x1, step):
            draw.line([(max(k, x0 + 4), y0 + 4 + max(0, x0 + 4 - k)), (min(k + (y1 - y0), x1 - 4), y0 + 4 + min(y1 - y0 - 8, x1 - 4 - k))],
                      fill=shade(60), width=1)
    elif kind == 2:  # dot screen
        step = int(rng.integers(6, 11))
        for y in range(y0 + 8, y1 - 8, step):
            for x in range(x0 + 8, x1 - 8, step):
                draw.ellipse([x, y, x + 2, y + 2], fill=shade(40))
    else:  # a dark shape on a flat ground
        cx, cy = (x0 + x1) // 2, (y0 + y1) // 2
        rx, ry = max(8, (x1 - x0) // 3), max(8, (y1 - y0) // 3)
        draw.ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=shade(35))
    if x1 - x0 > 160 and y1 - y0 > 140 and rng.random() < 0.7:
        bx, by = x0 + 20, y0 + 20
        bw, bh = min(150, x1 - x0 - 40), min(90, y1 - y0 - 40)
        draw.ellipse([bx, by, bx + bw, by + bh], fill=(255, 255, 255), outline=(0, 0, 0), width=2)
        for line in range(3):
            ly = by + bh // 3 + line * 12
            draw.rectangle([bx + bw // 5, ly, bx + bw - bw // 5, ly + 4], fill=(20, 20, 20))


def page(seed, size=PAGE, margins=(60, 70, 60, 95), number="centre", colour=False):
    """A page of panels inside `margins` (left, top, right, bottom)."""
    rng = np.random.default_rng(seed)
    image = Image.new("RGB", size, (255, 255, 255))
    draw = ImageDraw.Draw(image)
    left, top, right, bottom = margins
    width, height = size
    gutter = 18
    rows = int(rng.integers(2, 5))
    weights = rng.uniform(0.6, 1.6, rows)
    usable = height - top - bottom - gutter * (rows - 1)
    y = top
    for row in range(rows):
        row_height = int(usable * weights[row] / weights.sum())
        columns = int(rng.integers(1, 4))
        column_weights = rng.uniform(0.7, 1.5, columns)
        usable_width = width - left - right - gutter * (columns - 1)
        x = left
        for column in range(columns):
            panel_width = int(usable_width * column_weights[column] / column_weights.sum())
            tint = tuple(rng.uniform(0.45, 1.0, 3)) if colour else (1.0, 1.0, 1.0)
            _fill_panel(draw, rng, (x, y, x + panel_width, y + row_height), tint)
            x += panel_width + gutter
        y += row_height + gutter
    if number:
        # Two small dark "digits" in the bottom margin.
        ny = height - bottom + 38
        nx = width // 2 - 12 if number == "centre" else width - right - 40
        for digit in range(2):
            draw.rectangle([nx + digit * 14, ny, nx + digit * 14 + 9, ny + 15], fill=(25, 25, 25))
            draw.rectangle([nx + digit * 14 + 3, ny + 4, nx + digit * 14 + 6, ny + 7], fill=(255, 255, 255))
    return image


def strip(seed, width, height, spans, background):
    """A vertical strip: a panel for each (top, height) in `spans`, flat colour between."""
    rng = np.random.default_rng(seed)
    image = Image.new("RGB", (width, height), background)
    draw = ImageDraw.Draw(image)
    for top, span_height in spans:
        _fill_panel(draw, rng, (0, top, width - 1, top + span_height - 1), (1.0, 1.0, 1.0))
    return image


def write(directory):
    """Writes the corpus under `directory` and returns the lists of files the scenarios use."""
    pages_dir = os.path.join(directory, "pages")
    os.makedirs(pages_dir, exist_ok=True)

    def save(name, image, **options):
        path = os.path.join(pages_dir, name)
        image.save(path, **options)
        return os.path.abspath(path)

    plain = [save(f"plain{n:02d}.png", page(100 + n, number="centre" if n % 2 else "corner")) for n in range(6)]
    extra = [
        save("tight_margins.png", page(120, margins=(12, 14, 12, 40))),
        save("wide_margins.png", page(121, margins=(110, 120, 110, 150))),
        save("no_number.png", page(122, number=None)),
        save("dark.png", ImageOps.invert(page(123))),
        save("dark_no_number.png", ImageOps.invert(page(124, number=None))),
        save("grayscale_mode.png", page(125).convert("L")),
        save("jpeg_source.jpg", page(126), quality=90),
        save("small.png", page(127).resize((450, 675), Image.LANCZOS)),
        save("large.png", page(128).resize((1800, 2700), Image.BICUBIC)),
    ]

    two = Image.new("RGB", (1800, 1350), (255, 255, 255))
    two.paste(page(130), (0, 0)); two.paste(page(131), (900, 0))
    uneven = Image.new("RGB", (2000, 1400), (255, 255, 255))
    uneven.paste(page(132, margins=(0, 0, 0, 0), number=None), (150, 25))
    uneven.paste(page(133, margins=(0, 0, 0, 0), number=None), (1050, 25))
    three = Image.new("RGB", (2700, 1350), (255, 255, 255))
    for n in range(3):
        three.paste(page(134 + n), (900 * n, 0))
    spreads = [
        plain[0],
        save("spread.png", two),
        save("spread_uneven_margins.png", uneven),
        save("spread_very_wide.png", three),
        save("landscape_nearly_square.png", page(137, size=(1000, 900), margins=(40, 40, 40, 60))),
        save("spread_dark.png", ImageOps.invert(two)),
    ]

    colour = [
        save("colour_cover.png", page(140, colour=True, number=None)),
        plain[1],
        save("colour_inside.png", page(141, colour=True)),
        save("colour_tinted_scan.png", Image.merge("RGB", [band.point(lambda v, k=k: int(v * k)) for band, k in zip(page(142).split(), (1.0, 0.93, 0.82))])),
        save("colour_spread.png", Image.merge("RGB", [band.point(lambda v, k=k: int(v * k)) for band, k in zip(two.split(), (0.9, 1.0, 0.8))])),
    ]

    webtoon = {}
    for name, background, pieces in (
        ("webtoon_white", (255, 255, 255), [(2000, [(100, 600), (900, 1000)]), (2000, [(0, 1100)]), (2000, [(200, 400), (800, 1100)]), (1500, [(150, 1100)])]),
        ("webtoon_black", (0, 0, 0), [(2400, [(120, 700), (1000, 1100)]), (2400, [(300, 900), (1400, 800)])]),
        ("webtoon_tall_panels", (255, 255, 255), [(5200, [(100, 1100), (1300, 1100), (2500, 1100), (3700, 1100)]), (6000, [(200, 5500)])]),
    ):
        chapter = os.path.join(directory, name)
        os.makedirs(chapter, exist_ok=True)
        for n, (height, spans) in enumerate(pieces, 1):
            strip(200 + n, 800, height, spans, background).save(os.path.join(chapter, f"{n:03d}.png"))
        webtoon[name] = os.path.abspath(chapter)

    return {"pages": plain + extra, "spreads": spreads, "colour": colour, "webtoon": webtoon}


if __name__ == "__main__":
    import sys

    lists = write(sys.argv[1] if len(sys.argv) > 1 else "corpus")
    print({name: len(files) for name, files in lists.items()})
