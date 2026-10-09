"""Bookkeeping of the open-differences runner: it must not report a difference for the wrong reason."""

import unittest

import differences


class OpenDifferenceTests(unittest.TestCase):
    def test_ids_are_unique_and_every_kind_is_explained(self):
        ids = [case.id for case in differences.DIFFERENCES]
        self.assertEqual(len(ids), len(set(ids)))
        for case in differences.DIFFERENCES:
            self.assertIn(f'"{case.kind}"', differences.__doc__)

    def test_marks_survive_lossy_storage_and_name_their_page(self):
        for index in (0, 5, 11):
            jpeg = differences.encode(differences.marked_page(index), "JPEG", quality=85)
            self.assertEqual(differences.mark_of(differences.png(index)), index)
            self.assertEqual(differences.mark_of(jpeg), index)

    def test_every_book_names_an_author_unless_the_case_is_about_its_absence(self):
        for case in differences.DIFFERENCES:
            files, _ = case.build(None)
            metadata = [data for name, data in files.items() if name.endswith("ComicInfo.xml")]
            self.assertEqual(len(metadata), 1, case.id)
            if case.id not in ("META-2", "META-3"):
                self.assertIn(b"<Writer>", metadata[0], case.id)

    def test_a_reordered_book_is_reported_as_order_and_an_unreadable_mark_is_not(self):
        pages = [differences.png(0), differences.png(1)]
        book = {"pages": pages, "metadata": {}, "navigation": {}, "sides": [], "direction": "ltr"}
        legend = ["a.png", "b.png"]
        self.assertIsNone(differences.describe(None, book, book, legend))
        swapped = dict(book, pages=pages[::-1])
        self.assertIn("page order", differences.describe(None, book, swapped, legend))
        blank = differences.encode(differences.Image.new("L", (400, 600), 255), "PNG")
        self.assertIn("pixels differ", differences.describe(None, book, dict(book, pages=[pages[0], blank]), legend))


if __name__ == "__main__":
    unittest.main()
