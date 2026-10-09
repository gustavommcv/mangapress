"""The generated input books say what they claim to, before either tool reads them."""

import io
from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

import book_inputs


def opened(data):
    return Image.open(io.BytesIO(data))


class GeneratedInputTests(unittest.TestCase):
    def test_hand_written_pngs_keep_their_depth_size_and_samples(self):
        samples = np.arange(12, dtype=np.uint16).reshape(3, 4) * 5000
        image = opened(book_inputs.raw_png(samples, 4, 0, 16))
        self.assertEqual((image.mode, image.size), ("I;16", (4, 3)))
        self.assertEqual(np.asarray(image).tolist(), samples.tolist())
        # Three channels of sixteen bits are four pixels wide here, not twelve.
        self.assertEqual(opened(book_inputs.raw_png(samples.reshape(1, 12), 4, 2, 16)).size, (4, 1))
        levels = np.array([[0, 1, 2, 3, 3]], dtype=np.uint8)
        packed = opened(book_inputs.raw_png(levels, 5, 0, 2))
        self.assertEqual(np.asarray(packed.convert("L")).tolist(), [[0, 85, 170, 255, 255]])

    def test_the_four_channel_jpeg_is_the_luminance_and_chroma_kind_and_keeps_the_picture(self):
        picture = Image.new("RGB", (16, 16), (200, 60, 30))
        data = book_inputs.ycck_jpeg(picture)
        self.assertEqual(data[data.index(b"Adobe") + 11], 2)
        decoded = opened(data)
        self.assertEqual(decoded.mode, "CMYK")
        difference = np.abs(np.asarray(decoded.convert("RGB"), np.int16) - np.asarray(picture, np.int16))
        self.assertLessEqual(int(difference.max()), 3)

    def test_orientation_tags_are_written_and_do_not_move_pixels(self):
        page = book_inputs.lopsided()
        tagged = opened(book_inputs.oriented(page, 6, "PNG"))
        self.assertEqual(tagged.getexif()[0x0112], 6)
        self.assertEqual(tagged.size, page.size)

    def test_decision_colours_have_exactly_the_chroma_they_are_named_for(self):
        for chroma in ((150, 128), (106, 128), (128, 149), (128, 124), (134, 128)):
            colour = book_inputs.colour_with_chroma(*chroma)
            self.assertEqual(Image.new("RGB", (1, 1), colour).convert("YCbCr").getpixel((0, 0))[1:], chroma)

    def test_decision_pages_put_the_stated_number_of_pixels_off_neutral(self):
        _, blue, red = book_inputs.decision_page([(481, (138, 128))]).convert("YCbCr").split()
        self.assertEqual(blue.histogram()[138], 481)
        self.assertEqual(red.getextrema(), (128, 128))
        self.assertEqual(book_inputs.decision_page().convert("L").getextrema(), (30, 225))
        _, blue, red = book_inputs.decision_page(tint=(118, 136)).convert("YCbCr").split()
        self.assertEqual((blue.getextrema(), red.getextrema()), ((118, 118), (136, 136)))

    def test_every_book_opens_with_a_plain_page_and_has_unique_names(self):
        for name in ("geometry", "colour decision"):
            pages = book_inputs.BOOKS[name]()
            self.assertEqual(pages[0][0], "plain.png")
            self.assertEqual(len({page for page, _ in pages}), len(pages))
        self.assertEqual([opened(data).size for _, data in book_inputs.geometry()[1:]], book_inputs.GEOMETRY)

    def test_pages_are_written_in_one_chapter_in_the_order_given(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = book_inputs.write(Path(directory) / "Book", [("b.png", b"1"), ("a.png", b"2")])
            self.assertEqual([path.name for path in paths], ["000_b.png", "001_a.png"])
            self.assertEqual({path.parent.name for path in paths}, {"Pages"})
            self.assertEqual(sorted(path.name for path in paths), [path.name for path in paths])


if __name__ == "__main__":
    unittest.main()
