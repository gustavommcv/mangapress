"""Negative controls for semantic book comparisons and upstream isolation."""

import copy
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from PIL import Image

import books
import kcc_oracle


def image_bytes(shade=40, *, format="PNG", **options):
    output = io.BytesIO()
    Image.new("L", (8, 12), shade).save(output, format=format, **options)
    return output.getvalue()


class SemanticComparisonTests(unittest.TestCase):
    def setUp(self):
        self.book = {"pages": [image_bytes(40), image_bytes(160)], "metadata": {"title": ("Test",)},
                     "direction": "ltr", "sides": [("page-spread-left",), ("page-spread-right",)],
                     "navigation": {"ncx": (("Chapter", 0),), "nav": (("Chapter", 0),)},
                     "cover": image_bytes(40, format="JPEG", quality=85)}

    def test_identical_books_pass_and_empty_books_fail(self):
        books.compare_book(self.book, self.book)
        with self.assertRaisesRegex(ValueError, "empty"):
            books.compare_book({"pages": []}, {"pages": []})

    def test_removed_and_reordered_pages_fail(self):
        for pages in (self.book["pages"][:1], list(reversed(self.book["pages"]))):
            other = dict(self.book, pages=pages)
            with self.assertRaises(ValueError):
                books.compare_book(self.book, other)

    def test_metadata_direction_sides_navigation_and_cover_changes_fail(self):
        for key, value in (("metadata", {"title": ("Wrong",)}), ("direction", "rtl"),
                           ("sides", []), ("navigation", {}), ("cover", image_bytes(160, format="JPEG", quality=85))):
            with self.subTest(key=key), self.assertRaises(ValueError):
                books.compare_book(self.book, dict(self.book, **{key: value}))

    def test_passthrough_requires_source_bytes_not_just_identical_pixels(self):
        originals = self.book["pages"]
        other = copy.deepcopy(self.book)
        other["pages"][0] = image_bytes(40, compress_level=0)
        books.compare_book(self.book, other)
        with self.assertRaisesRegex(ValueError, "source bytes"):
            books.compare_book(self.book, other, originals=originals)

    def test_wrong_jpeg_quality_is_detected_on_flat_pixels(self):
        # Codec checks detect this even when decoded images are essentially identical.
        with self.assertRaisesRegex(ValueError, "quantization tables"):
            books.compare_image(image_bytes(format="JPEG", quality=85),
                                image_bytes(format="JPEG", quality=50), codec=True)

    def test_png_container_changes_do_not_hide_behind_equal_pixels(self):
        gray = image_bytes()
        output = io.BytesIO()
        Image.new("RGB", (8, 12), (40, 40, 40)).save(output, format="PNG")
        books.compare_image(gray, output.getvalue())
        with self.assertRaisesRegex(ValueError, "PNG depth/type"):
            books.compare_image(gray, output.getvalue(), codec=True)

    def test_only_the_documented_kindle_gif_to_png_difference_is_allowed(self):
        gif, png = image_bytes(format="GIF"), image_bytes()
        books.compare_image(gif, png, codec=True, kindle_png=True)
        for a, b, allowed in ((gif, png, False), (image_bytes(format="JPEG"), png, True)):
            with self.assertRaisesRegex(ValueError, "codec"):
                books.compare_image(a, b, codec=True, kindle_png=allowed)

    def test_a_single_color_pixel_cannot_pass_the_gray_noise_tolerance(self):
        image = Image.new("RGB", (8, 12), (40, 40, 40))
        image.putpixel((0, 0), (40, 40, 41))
        output = io.BytesIO()
        image.save(output, format="PNG")
        with self.assertRaisesRegex(ValueError, "classification"):
            books.compare_image(image_bytes(), output.getvalue())


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.members = {
            "mimetype": b"application/epub+zip",
            "META-INF/container.xml": '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="Book/content.opf"/></rootfiles></container>',
            "Book/content.opf": '''<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Test</dc:title></metadata><manifest>
<item id="p" href="Text/page.xhtml" media-type="application/xhtml+xml"/>
<item id="i" href="Images/page.png" media-type="image/png" properties="cover-image"/>
<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
</manifest><spine page-progression-direction="ltr"><itemref idref="p" properties="page-spread-left"/></spine></package>''',
            "Book/Text/page.xhtml": '<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="../Images/page.png"/></body></html>',
            "Book/Images/page.png": image_bytes(),
            "Book/toc.ncx": '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint><navLabel><text>Chapter</text></navLabel><content src="Text/page.xhtml"/></navPoint></navMap></ncx>',
            "Book/nav.xhtml": '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="Text/page.xhtml">Chapter</a></li></ol></nav></body></html>',
        }

    def read(self):
        path = self.directory / "fixture.epub"
        with zipfile.ZipFile(path, "w") as archive:
            for name, data in self.members.items():
                archive.writestr(name, data)
        return books.read_epub(path)

    def test_relative_links_resolve_and_navigation_has_a_real_page_index(self):
        book = self.read()
        self.assertEqual(book["navigation"], {"ncx": (("Chapter", 0),), "nav": (("Chapter", 0),)})
        self.assertEqual(book["pages"], [image_bytes()])

    def test_center_prefix_normalization_does_not_hide_a_different_side(self):
        original = self.members["Book/content.opf"]
        self.members["Book/content.opf"] = original.replace("page-spread-left", "page-spread-center")
        kcc_center = self.read()
        self.members["Book/content.opf"] = original.replace("page-spread-left", "rendition:page-spread-center")
        books.compare_book(kcc_center, self.read())
        self.members["Book/content.opf"] = original.replace("page-spread-left", "rendition:page-spread-right")
        with self.assertRaisesRegex(ValueError, "sides"):
            books.compare_book(kcc_center, self.read())

    def test_missing_image_or_navigation_resource_fails(self):
        for name in ("Book/Images/page.png", "Book/nav.xhtml"):
            with self.subTest(name=name):
                value = self.members.pop(name)
                with self.assertRaises(KeyError):
                    self.read()
                self.members[name] = value

    def test_disagreeing_navigation_labels_and_broken_targets_fail(self):
        original = self.members["Book/nav.xhtml"]
        for wrong in (original.replace(">Chapter<", ">Wrong<"), original.replace("Text/page.xhtml", "missing.xhtml")):
            self.members["Book/nav.xhtml"] = wrong
            with self.assertRaises(ValueError):
                self.read()

    def test_nested_navigation_preserves_parent_and_child_with_the_same_target(self):
        self.members["Book/toc.ncx"] = '''<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/">
<navMap><navPoint><navLabel><text>Volume</text></navLabel><content src="Text/page.xhtml"/>
<navPoint><navLabel><text>Chapter</text></navLabel><content src="Text/page.xhtml"/></navPoint>
</navPoint></navMap></ncx>'''
        self.members["Book/nav.xhtml"] = '''<html xmlns="http://www.w3.org/1999/xhtml"
xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol>
<li><a href="Text/page.xhtml">Volume</a><ol><li><a href="Text/page.xhtml">Chapter</a></li></ol></li>
</ol></nav></body></html>'''
        book = self.read()
        self.assertEqual(book["toc"], (("Volume", 0, (("Chapter", 0, ()),)),))
        self.assertEqual(book["navigation"]["ncx"], (("Volume", 0), ("Chapter", 0)))
        # The labels and targets are still identical after flattening. Only
        # keeping the tree can detect a lost parent/child relationship.
        self.members["Book/nav.xhtml"] = self.members["Book/nav.xhtml"].replace(
            '<ol><li><a href="Text/page.xhtml">Chapter</a></li></ol></li>',
            '</li><li><a href="Text/page.xhtml">Chapter</a></li>')
        with self.assertRaisesRegex(ValueError, "navigation disagree"):
            self.read()

    def test_empty_spine_or_missing_cover_fails(self):
        original = self.members["Book/content.opf"]
        for wrong in (original.replace('<itemref idref="p" properties="page-spread-left"/>', ""),
                      original.replace('properties="cover-image"', "")):
            self.members["Book/content.opf"] = wrong
            with self.assertRaises(ValueError):
                self.read()

    def test_cbz_uses_natural_order_and_rejects_empty_archives(self):
        path = self.directory / "fixture.cbz"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("Chapter 10/page10.png", image_bytes(160))
            archive.writestr("Chapter 2/page2.png", image_bytes(40))
        self.assertEqual(books.read_cbz(path)["pages"], [image_bytes(40), image_bytes(160)])
        with zipfile.ZipFile(path, "w"):
            pass
        with self.assertRaisesRegex(ValueError, "empty"):
            books.read_cbz(path)

    def test_external_and_escaping_image_links_fail(self):
        for target in ("https://example.org/image.png", "../../outside.png", "/outside.png"):
            with self.subTest(target=target), self.assertRaises(ValueError):
                books.member("Book/page.xhtml", target)

    def test_oracle_refuses_system_or_nonempty_temporary_roots_before_loading_kcc(self):
        temporary = self.directory / "temp"
        temporary.mkdir()
        with patch.object(kcc_oracle, "load_kcc") as load:
            with self.assertRaises(SystemExit):
                kcc_oracle.run_book("unused", self.directory / "other", temporary, [])
            (temporary / "existing").write_text("preserve", encoding="utf-8")
            with self.assertRaises(SystemExit):
                kcc_oracle.run_book("unused", self.directory, temporary, [])
            load.assert_not_called()
        self.assertEqual((temporary / "existing").read_text(encoding="utf-8"), "preserve")

    def test_oracle_isolates_upstream_cleanup_and_worker_imports(self):
        temporary = self.directory / "temp"
        temporary.mkdir()
        checkout = self.directory / "reference"
        with patch.dict(os.environ), patch.object(tempfile, "tempdir", None):
            with patch.object(kcc_oracle, "load_kcc") as load:
                def verify_isolation(path):
                    self.assertEqual(Path(path), checkout)
                    self.assertEqual(tempfile.gettempdir(), str(temporary.resolve()))
                    for variable in ("TMPDIR", "TEMP", "TMP"):
                        self.assertEqual(os.environ[variable], str(temporary.resolve()))
                    self.assertEqual(os.environ["MANGAPRESS_PARITY_KCC"], str(checkout.resolve()))
                    cli = unittest.mock.Mock()
                    cli.main.return_value = 0
                    return "12.0.0", cli, None, None
                load.side_effect = verify_isolation
                with self.assertRaises(SystemExit) as result:
                    kcc_oracle.run_book(str(checkout), self.directory, temporary, ["input"])
                self.assertEqual(result.exception.code, 0)

    def test_unused_codec_stub_cannot_claim_to_have_optimized_an_image(self):
        with self.assertRaisesRegex(RuntimeError, "does not exercise MozJPEG"):
            kcc_oracle.unavailable_codec(b"image")


if __name__ == "__main__":
    unittest.main()
