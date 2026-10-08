"""Negative controls for intended book content, separate from format conformance."""

import copy
from dataclasses import replace
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET
import zipfile

from PIL import Image

import check  # Adds the shared tools directory for the standalone test command.
import cases


def encoded_image(format="JPEG", size=(8, 12), colour=40, **options):
    output = io.BytesIO()
    Image.new("RGB" if isinstance(colour, tuple) else "L", size, colour).save(output, format=format, **options)
    return output.getvalue()


def document(size=(8, 12)):
    return ET.fromstring(f'''<html xmlns="http://www.w3.org/1999/xhtml"><head>
<meta name="viewport" content="width={size[0]}, height={size[1]}"/></head><body>
<img width="{size[0]}" height="{size[1]}"/><img width="{size[0]}" height="{size[1]}"/></body></html>''')


class BookAssertionTests(unittest.TestCase):
    def setUp(self):
        self.case = cases.Case("test")
        self.book = {
            "pages": [encoded_image(quality=85)] * 4,
            "documents": [document() for _ in range(4)],
            "cover": encoded_image(quality=85),
            "toc": (("Chapter 2", 0, ()), ("Chapter 10", 2, ())),
            "direction": "ltr", "sides": [("page-spread-left",), ("page-spread-right",)] * 2,
            "metadata": {"title": ("Synthetic Series Vol. 02 #007",), "creators": ("Ada", "Zed"),
                         "language": ("en-US",), "description": ("Panels & ramps.",)},
            "package": ET.fromstring('''<package xmlns="http://www.idpf.org/2007/opf"><metadata>
<meta name="original-resolution" content="1072x1448"/>
<meta name="primary-writing-mode" content="horizontal-lr"/>
<meta property="rendition:layout">pre-paginated</meta>
<meta property="rendition:spread">landscape</meta></metadata></package>'''),
        }

    def verify(self, case=None, originals=()):
        # Only archive reading is substituted here. The images and XML assertions
        # are real; shared-reader archive tests and the CLI suite cover ZIP wiring.
        with patch.object(cases, "read_epub", return_value=self.book):
            cases.verify_book(Path("unused.epub"), case or self.case, originals)

    def test_valid_content_passes_and_missing_pages_fail(self):
        self.verify()
        self.book["pages"].pop()
        with self.assertRaisesRegex(ValueError, "spine page count"):
            self.verify()

    def test_wrong_toc_label_or_real_target_fails(self):
        for toc in ((("Wrong", 0, ()), ("Chapter 10", 2, ())),
                    (("Chapter 2", 0, ()), ("Chapter 10", 3, ()))):
            self.book["toc"] = toc
            with self.subTest(toc=toc), self.assertRaisesRegex(ValueError, "TOC"):
                self.verify()

    def test_flattened_nested_toc_or_wrong_volume_target_fails(self):
        self.book["pages"] *= 2
        self.book["pages"] = self.book["pages"][:6]
        self.book["documents"] = [document() for _ in range(6)]
        self.book["sides"] *= 2
        self.book["sides"] = self.book["sides"][:6]
        case = replace(self.case, scenario="nested", codec="PNG")
        self.book["pages"] = [encoded_image("PNG", colour=40 + index * 20) for index in range(6)]
        originals = tuple(self.book["pages"])
        self.book["toc"] = (("Volume 2", 0, (("Chapter 1", 0, ()), ("Chapter 10", 2, ()))),
                            ("Volume 10", 4, (("Chapter 1", 4, ()),)))
        self.verify(case, originals)
        self.book["pages"][0], self.book["pages"][1] = self.book["pages"][1], self.book["pages"][0]
        with self.assertRaisesRegex(ValueError, "nested pixels"):
            self.verify(case, originals)
        self.book["pages"][0], self.book["pages"][1] = self.book["pages"][1], self.book["pages"][0]
        for toc in ((("Chapter 1", 0, ()), ("Chapter 10", 2, ()), ("Chapter 1", 4, ())),
                    (("Volume 2", 0, (("Chapter 1", 0, ()), ("Chapter 10", 2, ()))),
                     ("Volume 10", 0, (("Chapter 1", 4, ()),)))):
            self.book["toc"] = toc
            with self.subTest(toc=toc), self.assertRaisesRegex(ValueError, "TOC"):
                self.verify(case, originals)

    def test_wrong_metadata_direction_and_sides_fail(self):
        for key in ("title", "creators", "language", "description"):
            original = self.book["metadata"][key]
            self.book["metadata"][key] = ("Wrong",)
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, key):
                self.verify()
            self.book["metadata"][key] = original
        self.book["direction"] = "rtl"
        with self.assertRaisesRegex(ValueError, "direction"):
            self.verify()
        self.book["direction"] = "ltr"
        self.book["sides"][0] = ("page-spread-center",)
        with self.assertRaisesRegex(ValueError, "placement"):
            self.verify()

    def test_bookmark_targets_follow_split_pages_and_cover_stays_whole(self):
        self.book["pages"].append(encoded_image(quality=85))
        self.book["documents"].append(document())
        self.book["direction"] = "rtl"
        self.book["sides"] = [(f"page-spread-{side}",) for side in ("right", "left", "right", "left", "right")]
        self.book["package"].find("opf:metadata/opf:meta[@name='primary-writing-mode']", cases.NS).set(
            "content", "horizontal-rl")
        self.book["toc"] = (("Spread & start", 0, ()), ("After spread <2>", 2, ()), ("End", 4, ()))
        self.book["cover"] = encoded_image(size=(256, 192), quality=85)
        case = replace(self.case, scenario="bookmarks", flags=("-m",))
        self.verify(case)
        toc = self.book["toc"]
        self.book["toc"] = (("Spread & start", 0, ()), ("After spread <2>", 1, ()), ("End", 3, ()))
        with self.assertRaisesRegex(ValueError, "TOC"):
            self.verify(case)
        self.book["toc"] = toc
        self.book["cover"] = encoded_image(size=(128, 192), quality=85)
        with self.assertRaisesRegex(ValueError, "unsplit source"):
            self.verify(case)

    def test_collection_name_type_position_and_refinement_target_are_checked(self):
        metadata = self.book["package"].find("opf:metadata", cases.NS)
        for node in list(metadata):
            if "name" in node.attrib:
                metadata.remove(node)
        for properties, text in (({"property": "belongs-to-collection", "id": "series"}, "Synthetic Series"),
                                 ({"property": "collection-type", "refines": "#series"}, "series"),
                                 ({"property": "group-position", "refines": "#series"}, "2.7")):
            ET.SubElement(metadata, "{http://www.idpf.org/2007/opf}meta", properties).text = text
        case = replace(self.case, profile="KoLC", original_resolution=None, kindle=False)
        self.verify(case)
        for property in ("belongs-to-collection", "collection-type", "group-position"):
            node = metadata.find(f"opf:meta[@property='{property}']", cases.NS)
            original = node.text
            node.text = "Wrong"
            with self.subTest(property=property), self.assertRaises(ValueError):
                self.verify(case)
            node.text = original
        metadata.find("opf:meta[@property='group-position']", cases.NS).set("refines", "#other")
        with self.assertRaisesRegex(ValueError, "series position"):
            self.verify(case)

    def test_missing_resolution_and_wrong_layout_fail(self):
        for key, context in (("original-resolution", "resolution"), ("primary-writing-mode", "writing mode")):
            node = self.book["package"].find(f"opf:metadata/opf:meta[@name='{key}']", cases.NS)
            original = node.attrib["content"]
            node.attrib["content"] = "Wrong"
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, context):
                self.verify()
            node.attrib["content"] = original
        node = self.book["package"].find("opf:metadata/opf:meta[@property='rendition:layout']", cases.NS)
        node.text = "reflowable"
        with self.assertRaisesRegex(ValueError, "fixed layout"):
            self.verify()

    def test_custom_target_cannot_keep_kindle_original_resolution(self):
        with self.assertRaisesRegex(ValueError, "original-resolution"):
            self.verify(replace(self.case, original_resolution=None))

    def test_wrong_viewport_and_hidden_image_dimensions_fail(self):
        viewport = self.book["documents"][0].find("x:head/x:meta", cases.NS)
        viewport.attrib["content"] = "width=9, height=12"
        with self.assertRaisesRegex(ValueError, "viewport"):
            self.verify()
        viewport.attrib["content"] = "width=8, height=12"
        self.book["documents"][0].findall(".//x:img", cases.NS)[1].attrib["height"] = "13"
        with self.assertRaisesRegex(ValueError, "image dimensions"):
            self.verify()

    def test_wrong_stretched_size_codec_and_jpeg_tables_fail(self):
        with self.assertRaisesRegex(ValueError, "full-size image"):
            self.verify(replace(self.case, flags=("--stretch",), target=(9, 12)))
        self.book["pages"][0] = encoded_image("PNG")
        with self.assertRaisesRegex(ValueError, "codec"):
            self.verify()
        self.book["pages"][0] = encoded_image(quality=50)
        with self.assertRaisesRegex(ValueError, "JPEG quality"):
            self.verify()

    def test_a_profile_cannot_silently_exceed_its_palette(self):
        output = io.BytesIO()
        image = Image.new("L", (8, 12))
        image.putdata(list(range(16)) * 6)
        image.save(output, format="PNG")
        self.book["pages"] = [output.getvalue()] * 4
        for profile in ("K1", "K2"):
            with self.subTest(profile=profile), self.assertRaisesRegex(ValueError, "grayscale palette"):
                self.verify(replace(self.case, profile=profile, codec="PNG"))

    def test_reencoded_passthrough_with_identical_pixels_fails(self):
        originals = tuple(self.book["pages"])
        self.verify(replace(self.case, scenario="passthrough", codec=None), originals)
        self.book["pages"][0] = encoded_image(quality=85, comment=b"reencoded")
        with self.assertRaisesRegex(ValueError, "unchanged source bytes"):
            self.verify(replace(self.case, scenario="passthrough", codec=None), originals)

    def test_lost_colors_and_wrong_external_cover_fail(self):
        self.book["pages"] = [encoded_image(colour=(40, 40, 40), quality=85)] * 4
        with self.assertRaisesRegex(ValueError, "lost.*colors"):
            self.verify(replace(self.case, colour=True))
        self.book["pages"] = [encoded_image(quality=85)] * 4
        self.book["cover"] = encoded_image(size=(128, 192), quality=85)
        with self.assertRaisesRegex(ValueError, "external red cover"):
            self.verify(replace(self.case, scenario="cover"))


class EventAndFixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name).resolve()
        self.book = self.directory / "book.epub"
        self.book.write_bytes(b"book")
        self.case = cases.Case("test")
        self.events = [
            {"tool": "mangapress", "protocol_version": 1, "sequence": 1, "type": "stage",
             "stage": "plan", "state": "completed", "format": "epub", "profile": "K11",
             "width": 1072, "height": 1448, "output_path": str(self.book)},
            {"tool": "mangapress", "protocol_version": 1, "sequence": 2, "type": "result",
             "status": "completed", "dry_run": False, "written": True, "format": "epub", "profile": "K11",
             "width": 1072, "height": 1448, "output_path": str(self.book), "source_pages": 4,
             "output_pages": 4, "bytes": 4},
        ]

    def verify(self, events=None, dry_run=False):
        cases.verify_events("\n".join(json.dumps(event) for event in (events or self.events)),
                            self.case, self.book, dry_run)

    def test_correct_plan_and_result_pass_but_wrong_targets_counts_and_paths_fail(self):
        self.verify()
        for index, key, value in ((0, "width", 999), (1, "height", 999), (0, "profile", "KDX"),
                                  (1, "format", "cbz"), (1, "source_pages", 5), (1, "output_pages", 5),
                                  (1, "bytes", 0), (0, "output_path", "old.epub"), (1, "sequence", 1),
                                  (1, "written", False), (1, "protocol_version", 2)):
            events = copy.deepcopy(self.events)
            events[index][key] = value
            with self.subTest(index=index, key=key), self.assertRaises(ValueError):
                self.verify(events)

    def test_dry_run_contract_and_missing_or_extra_plan_are_checked(self):
        events = copy.deepcopy(self.events)
        events[-1].update(dry_run=True, written=False)
        events[-1].pop("output_pages")
        events[-1].pop("bytes")
        self.verify(events, dry_run=True)
        for invalid in (self.events[1:], self.events + [self.events[0]], [], [None]):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                cases.verify_events("\n".join(json.dumps(event) for event in invalid), self.case, self.book, False)

    def test_bmp_control_requires_its_specific_rejection_not_a_crash_or_success(self):
        book = self.directory / "refused.epub"
        explanation = "BMP pages cannot be embedded in EPUB without processing. Remove --noprocessing or choose CBZ output."
        error = {"tool": "mangapress", "protocol_version": 1, "sequence": 1, "type": "error",
                 "code": "page_processing_failed", "stage": "process", "severity": "error", "diagnostic": explanation}
        cases.verify_bmp_rejection(1, json.dumps(error), explanation, book)
        for status, stdout, stderr in ((0, json.dumps(error), explanation),
                                       (2, json.dumps(error), explanation), (1, "not JSON", explanation),
                                       (1, json.dumps(dict(error, code="input_not_found")), explanation),
                                       (1, json.dumps(dict(error, diagnostic="Other processing error")), explanation),
                                       (1, json.dumps(error), "Unrelated stderr")):
            with self.subTest(status=status, stdout=stdout), self.assertRaises(ValueError):
                cases.verify_bmp_rejection(status, stdout, stderr, book)
        result = dict(error, type="result")
        stream = json.dumps(result) + "\n" + json.dumps(dict(error, sequence=2))
        with self.assertRaisesRegex(ValueError, "result or book"):
            cases.verify_bmp_rejection(1, stream, explanation, book)
        book.write_bytes(b"unexpected book")
        with self.assertRaisesRegex(ValueError, "result or book"):
            cases.verify_bmp_rejection(1, json.dumps(error), explanation, book)

    def test_all_cases_have_the_expected_input_count_and_preserve_fixture_bytes(self):
        self.assertEqual(len(cases.CASES), len({case.name for case in cases.CASES}))
        for case in cases.CASES:
            directory = self.directory / case.name
            directory.mkdir()
            source, flags, originals, unchanged = cases.prepare_case(directory, case)
            with self.subTest(case=case.name):
                self.assertEqual(len(originals), cases.page_counts(case)[0])
                self.assertEqual(cases.snapshot(unchanged), unchanged)
                if case.archive:
                    with zipfile.ZipFile(source) as archive:
                        self.assertIn("ComicInfo.xml", archive.namelist())
                        self.assertEqual(set(originals), {archive.read(name) for name in archive.namelist()
                                                          if name != "ComicInfo.xml"})
                if case.scenario == "passthrough":
                    self.assertEqual([Image.open(io.BytesIO(data)).format for data in originals],
                                     ["JPEG", "GIF", "WEBP", "PNG"])
                if case.scenario == "cover":
                    self.assertEqual(flags[0], "--cover")
                    self.assertIn(Path(flags[1]), unchanged)


if __name__ == "__main__":
    unittest.main()
