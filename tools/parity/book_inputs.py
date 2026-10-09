"""Generated books whose pages differ in how they are stored, shaped or tinted.

The small chapter fixture answers "is the book put together the same way".
These answer "does each tool read the same pixels from this file, cut it the
same way, and call it gray or colour for the same reason". Every page is
drawn here: nothing is, or comes from, a real comic.

Each book is one chapter folder, so that both tools agree on which page is
the first one, and opens with an ordinary gray page, which becomes the cover.
"""

import io
import struct
import zlib

import numpy as np
from PIL import Image, ImageChops, ImageCms, ImageDraw

import make_corpus

# The decision vectors below are counted in pixels of a page this size.
SMALL = (400, 600)


def encode(image, fmt, **options):
    data = io.BytesIO()
    image.save(data, fmt, **options)
    return data.getvalue()


def raw_png(samples, width, colour_type, depth):
    """A PNG written by hand, for the bit depths Pillow reads but does not write.

    `samples` holds one row per line, already interleaved by channel.
    """
    height = samples.shape[0]
    if depth == 16:
        rows = [b"\x00" + samples[y].astype(">u2").tobytes() for y in range(height)]
    else:
        per_byte = 8 // depth
        rows = []
        for y in range(height):
            line = samples[y].astype(np.uint8)
            line = np.concatenate([line, np.zeros(-len(line) % per_byte, np.uint8)]).reshape(-1, per_byte)
            packed = np.zeros(len(line), np.uint8)
            for position in range(per_byte):
                packed |= line[:, position] << (8 - depth * (position + 1))
            rows.append(b"\x00" + packed.tobytes())

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    header = struct.pack(">IIBBBBB", width, height, depth, colour_type, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b"")


def ycck_jpeg(image):
    """A four-channel JPEG whose Adobe marker says luminance, two chroma and black.

    Pillow only writes the plain four-ink kind, storing each channel inverted.
    Handing it luminance and chroma in place of the inks, then changing the
    marker's transform byte, gives the other kind with known content. The
    luminance and chroma are those of the picture's negative, because that is
    what the inks are once a reader has undone the conversion.
    """
    ycc = np.asarray(ImageChops.invert(image.convert("RGB")).convert("YCbCr"))
    stored = np.dstack([255 - ycc, np.zeros(ycc.shape[:2], np.uint8)])
    data = bytearray(encode(Image.frombytes("CMYK", image.size, stored.tobytes()), "JPEG", quality=90))
    transform = data.index(b"Adobe") + 11
    if data[transform] != 0:
        raise ValueError("expected a marker declaring four plain inks")
    data[transform] = 2
    return bytes(data)


def oriented(image, orientation, fmt, **options):
    exif = Image.Exif()
    exif[0x0112] = orientation
    return encode(image, fmt, exif=exif, **options)


def halftone():
    """Saturated dots at a pitch of one and two pixels: the hardest case for chroma kept at half size."""
    rng = np.random.default_rng(5)
    array = np.full((1350, 900, 3), 255, np.uint8)
    for top in range(0, 1350, 150):
        first, second = rng.integers(0, 256, 3), rng.integers(0, 256, 3)
        pitch = 1 + (top // 150) % 2
        ys, xs = np.mgrid[top:top + 150, 0:900]
        dots = ((ys // pitch) + (xs // pitch)) % 2 == 0
        array[top:top + 150][dots] = first
        array[top:top + 150][~dots] = second
    return Image.fromarray(array)


def lopsided():
    """A page that looks different under each of the eight orientations."""
    image = Image.new("RGB", (600, 900), "white")
    draw = ImageDraw.Draw(image)
    draw.rectangle((30, 30, 569, 869), outline=(0, 0, 0), width=8)
    draw.rectangle((60, 60, 260, 200), fill=(20, 20, 20))
    draw.rectangle((60, 760, 540, 800), fill=(120, 120, 120))
    return image


def decoded_inputs():
    """One page per way of storing an image that a scan or a download really arrives in."""
    gray_rgb = make_corpus.page(301)
    gray = gray_rgb.convert("L")
    colour = make_corpus.page(302, colour=True)
    rng = np.random.default_rng(11)
    srgb = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
    other = ImageCms.ImageCmsProfile(ImageCms.createProfile("XYZ")).tobytes()
    width, height = gray.size

    gray16 = np.asarray(gray, np.uint16) * 257
    # Low bytes that are not a copy of the high byte: rounding and truncating to eight bits then disagree.
    colour16 = np.asarray(colour, np.uint16) * 257 - rng.integers(0, 128, (height, width, 3)).astype(np.uint16) * (np.asarray(colour) > 0)
    fade16 = np.tile(np.linspace(0, 65535, width, dtype=np.uint16), (height, 1))
    fade = Image.fromarray(np.tile(np.linspace(0, 255, width, dtype=np.uint8), (height, 1)))
    indexed = colour.quantize(64)
    commonest = int(np.bincount(np.asarray(indexed).ravel()).argmax())
    clear = np.asarray(gray) > 250
    opaque = np.where(clear, 0, 255).astype(np.uint8)
    hiding_black, hiding_red = np.asarray(colour).copy(), np.asarray(gray_rgb).copy()
    hiding_black[clear], hiding_red[clear] = (0, 0, 0), (255, 40, 40)
    tinted = Image.merge("RGB", [band.point(lambda v, k=k: int(v * k)) for band, k in zip(gray_rgb.split(), (1.0, 0.985, 0.96))])
    noisy = Image.fromarray(np.clip(np.asarray(gray_rgb, np.int16) + rng.integers(-3, 4, (height, width, 3)), 0, 255).astype(np.uint8))
    black_ink = Image.merge("CMYK", [Image.new("L", gray.size, 0)] * 3 + [gray.point(lambda v: 255 - v)])
    marked = lopsided()

    pages = [
        ("plain.png", encode(gray_rgb, "PNG")),
        ("png_rgb_srgb_profile.png", encode(colour, "PNG", icc_profile=srgb)),
        ("jpeg_rgb_other_profile.jpg", encode(colour, "JPEG", quality=90, icc_profile=other)),
        ("jpeg_gray.jpg", encode(gray, "JPEG", quality=90)),
        ("jpeg_four_inks_black_only.jpg", encode(black_ink, "JPEG", quality=90)),
        ("jpeg_four_inks_colour.jpg", encode(colour.convert("CMYK"), "JPEG", quality=90)),
        ("jpeg_luminance_chroma_black.jpg", ycck_jpeg(colour)),
        ("png_rgb_16bit.png", raw_png(colour16.reshape(height, -1), width, 2, 16)),
        ("png_gray_alpha_16bit.png", raw_png(np.dstack([gray16, fade16]).reshape(height, -1), width, 4, 16)),
        ("png_rgba_16bit.png", raw_png(np.dstack([colour16, fade16]).reshape(height, -1), width, 6, 16)),
        ("png_palette.png", encode(indexed, "PNG")),
        ("png_palette_one_clear_entry.png", encode(indexed, "PNG", transparency=commonest)),
        ("png_palette_alpha_per_entry.png", encode(indexed, "PNG", transparency=bytes(entry * 4 % 256 for entry in range(64)))),
        ("png_palette_of_grays.png", encode(gray.quantize(16), "PNG")),
        ("png_1bit.png", encode(gray.convert("1"), "PNG")),
        ("png_gray_2bit.png", raw_png(np.asarray(gray) >> 6, width, 0, 2)),
        ("png_gray_4bit.png", raw_png(np.asarray(gray) >> 4, width, 0, 4)),
        ("png_gray_alpha.png", encode(Image.merge("LA", [gray, fade]), "PNG")),
        ("png_rgba_clear_over_black.png", encode(Image.fromarray(np.dstack([hiding_black, opaque])), "PNG")),
        ("png_rgba_gray_art_clear_over_red.png", encode(Image.fromarray(np.dstack([hiding_red, opaque])), "PNG")),
        ("png_gray_one_clear_level.png", encode(gray, "PNG", transparency=255)),
        ("png_rgb_one_clear_colour.png", encode(colour, "PNG", transparency=(255, 255, 255))),
        ("jpeg_chroma_full.jpg", encode(colour, "JPEG", quality=85, subsampling=0)),
        ("jpeg_chroma_half_width.jpg", encode(colour, "JPEG", quality=85, subsampling=1)),
        ("jpeg_chroma_half_both.jpg", encode(colour, "JPEG", quality=85, subsampling=2)),
        ("jpeg_chroma_quarter_width.jpg", encode(colour, "JPEG", quality=85, subsampling="4:1:1")),
        ("jpeg_progressive.jpg", encode(colour, "JPEG", quality=85, subsampling=2, progressive=True)),
        ("jpeg_colour_halftone.jpg", encode(halftone(), "JPEG", quality=90, subsampling=2)),
        ("jpeg_gray_art_faint_tint.jpg", encode(tinted, "JPEG", quality=80, subsampling=2)),
        ("jpeg_gray_art_chroma_noise.jpg", encode(noisy, "JPEG", quality=70, subsampling=2)),
        ("gif.gif", encode(gray, "GIF")),
        ("gif_one_clear_entry.gif", encode(indexed, "GIF", transparency=commonest)),
        ("gif_two_frames.gif", encode(gray, "GIF", save_all=True, append_images=[gray.point(lambda v: 255 - v)], duration=100)),
        ("webp_lossy_colour.webp", encode(colour, "WEBP", quality=80)),
        ("webp_lossless_gray_art.webp", encode(gray_rgb, "WEBP", lossless=True)),
        ("webp_lossy_alpha.webp", encode(Image.merge("RGBA", list(colour.split()) + [fade]), "WEBP", quality=80)),
        ("webp_two_frames.webp", encode(colour, "WEBP", save_all=True, append_images=[gray_rgb], duration=100, lossless=True)),
        ("webp_lossy_colour_halftone.webp", encode(halftone(), "WEBP", quality=90)),
        ("png_content_named_jpg.jpg", encode(gray_rgb, "PNG")),
        ("jpeg_content_named_png.png", encode(gray_rgb, "JPEG", quality=90)),
    ]
    # Neither tool turns a page by its orientation tag; a tool that started to would show here.
    pages += [(f"jpeg_orientation_{value}.jpg", oriented(marked, value, "JPEG", quality=95)) for value in (2, 3, 6, 8)]
    pages += [("png_orientation_6.png", oriented(marked, 6, "PNG")), ("webp_orientation_6.webp", oriented(marked, 6, "WEBP", lossless=True))]
    return pages


# Kindle 11 is 1072 x 1448. The groups are: almost nothing; one very long side;
# around square; around the ratios where a wide page stops being one page and
# where it stops being cut in two; twice the screen at the heights where
# filling the screen gives way to fitting inside it; and around the screen itself.
GEOMETRY = [(1, 1), (2, 2), (3, 7), (16, 24), (1, 600), (600, 1), (3000, 1000), (400, 8000),
            (1000, 1000), (1000, 999), (999, 1000), (1159, 1000), (1160, 1000), (1161, 1000),
            (1799, 1000), (1800, 1000), (1801, 1000),
            (2144, 2863), (2144, 2864), (2144, 2896), (2144, 2928), (2144, 2929),
            (1071, 1447), (1072, 1448), (1073, 1451), (901, 1351), (536, 724)]


def geometry():
    base = make_corpus.page(501).convert("L")
    pages = [("plain.png", encode(make_corpus.page(301), "PNG"))]
    return pages + [(f"{width}x{height}.png", encode(base.resize((width, height), Image.LANCZOS), "PNG")) for width, height in GEOMETRY]


def colour_with_chroma(blue, red):
    """An RGB colour of middling brightness whose blue and red chroma are exactly these."""
    for luminance in range(110, 150):
        r, b = round(luminance + 1.402 * (red - 128)), round(luminance + 1.772 * (blue - 128))
        if not (0 <= r <= 255 and 0 <= b <= 255):
            continue
        for g in range(max(0, luminance - 40), min(255, luminance + 40)):
            if Image.new("RGB", (1, 1), (r, g, b)).convert("YCbCr").getpixel((0, 0))[1:] == (blue, red):
                return r, g, b
    raise ValueError(f"no colour with chroma {blue}, {red}")


def decision_page(patches=(), tint=None):
    """A neutral page of low contrast (levels 30 to 225), with some pixels given a chroma.

    The contrast is low enough that a page taken for gray is visibly stretched
    to the full range and a page taken for colour is not: the output shows
    which decision was made without asking either tool.
    """
    width, height = SMALL
    array = np.full((height, width, 3), 225, np.uint8)
    array[60:540, 40:360] = 30
    array[100:500, 80:320] = 225
    if tint:
        ycc = np.asarray(Image.fromarray(array).convert("YCbCr")).copy()
        ycc[:, :, 1], ycc[:, :, 2] = tint
        array = np.asarray(Image.fromarray(ycc, "YCbCr").convert("RGB")).copy()
    flat = array.reshape(-1, 3)
    start = 150 * width + 100
    for count, chroma in patches:
        flat[start:start + count] = colour_with_chroma(*chroma)
        start += count + width * 3
    return Image.fromarray(array)


def colour_decision():
    """Pages on each side of every boundary of the gray-or-colour decision.

    Neutral chroma is 128. The decision looks at the lowest and highest chroma
    present, three times: over all pixels (far enough is 22 away), without the
    0.2% most extreme at each end (10 away), and without the 3% most extreme
    (4 away). Unless colour was asked for, a page whose chroma spans fewer
    than 7 values is gray before any of that; if colour was asked for, a page
    whose chroma lies wholly on one side of neutral is colour.
    """
    pixels = SMALL[0] * SMALL[1]
    two_per_mille, three_percent, half = pixels * 2 // 1000, pixels * 3 // 100, pixels // 2 - 150 * SMALL[0]
    vectors = [
        ("one_pixel_blue_150", [(1, (150, 128))]), ("one_pixel_blue_149", [(1, (149, 128))]),
        ("one_pixel_blue_106", [(1, (106, 128))]), ("one_pixel_blue_107", [(1, (107, 128))]),
        ("one_pixel_red_150", [(1, (128, 150))]), ("one_pixel_red_149", [(1, (128, 149))]),
        ("just_over_0.2_percent_blue_138", [(two_per_mille + 1, (138, 128))]),
        ("exactly_0.2_percent_blue_138", [(two_per_mille, (138, 128))]),
        ("just_over_0.2_percent_blue_137", [(two_per_mille + 1, (137, 128))]),
        ("just_over_3_percent_red_132", [(three_percent + 1, (128, 132))]),
        ("exactly_3_percent_red_132", [(three_percent, (128, 132))]),
        ("just_over_3_percent_red_131", [(three_percent + 1, (128, 131))]),
        ("just_over_3_percent_red_124", [(three_percent + 1, (128, 124))]),
        ("just_over_3_percent_red_125", [(three_percent + 1, (128, 125))]),
        ("half_page_blue_134_span_6", [(half, (134, 128))]), ("half_page_blue_135_span_7", [(half, (135, 128))]),
    ]
    pages = [("plain.png", encode(decision_page(), "PNG"))]
    pages += [(f"{name}.png", encode(decision_page(patches), "PNG")) for name, patches in vectors]
    pages += [("whole_page_tinted_far.png", encode(decision_page(tint=(118, 136)), "PNG")),
              ("whole_page_tinted_near.png", encode(decision_page(tint=(126, 130)), "PNG"))]
    return pages


BOOKS = {"decoded inputs": decoded_inputs, "geometry": geometry, "colour decision": colour_decision}


def write(directory, pages, chapter="Pages"):
    """Write the pages into one chapter, numbered so that every sort agrees. Returns them in reading order."""
    folder = directory / chapter
    folder.mkdir(parents=True)
    paths = []
    for number, (name, data) in enumerate(pages):
        path = folder / f"{number:03d}_{name}"
        path.write_bytes(data)
        paths.append(path)
    return paths
