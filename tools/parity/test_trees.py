import io
from pathlib import Path
import tempfile
import unittest
import zipfile

from PIL import Image

from trees import INFO, marked_page, tree, write_source


class TreeTests(unittest.TestCase):
    def test_a_tree_has_one_marked_page_per_name_and_a_comic_info(self):
        files, legend = tree(["Ch 1/001.png", "Ch 1/002.png"])(None)

        self.assertEqual(legend, ["Ch 1/001.png", "Ch 1/002.png"])
        self.assertEqual(sorted(files), ["Ch 1/001.png", "Ch 1/002.png", "ComicInfo.xml"])
        self.assertEqual(files["ComicInfo.xml"], INFO.encode())
        with Image.open(io.BytesIO(files["Ch 1/002.png"])) as page:
            self.assertEqual(page.size, marked_page(1).size)

    def test_a_tree_can_leave_the_comic_info_out(self):
        files, _ = tree(["a/001.png"], info=None)(None)

        self.assertEqual(sorted(files), ["a/001.png"])

    def test_extra_files_follow_the_pages_in_the_legend(self):
        files, legend = tree(["a/001.png"], extra={"a/note.txt": b"x"})(None)

        self.assertEqual(legend, ["a/001.png", "a/note.txt"])
        self.assertEqual(files["a/note.txt"], b"x")


class WriteSourceTests(unittest.TestCase):
    def test_a_folder_keeps_the_layout(self):
        with tempfile.TemporaryDirectory() as work:
            source = write_source(Path(work), {"v1/c1/p1.png": b"a", "ComicInfo.xml": b"b"}, archive=False)

            self.assertTrue(source.is_dir())
            self.assertEqual((source / "v1/c1/p1.png").read_bytes(), b"a")
            self.assertEqual((source / "ComicInfo.xml").read_bytes(), b"b")

    def test_an_archive_keeps_the_layout_in_the_given_order(self):
        with tempfile.TemporaryDirectory() as work:
            source = write_source(Path(work), {"v1/c1/p1.png": b"a", "v1/c1/p2.png": b"b"}, archive=True)

            self.assertEqual(source.suffix, ".cbz")
            with zipfile.ZipFile(source) as book:
                self.assertEqual(book.namelist(), ["v1/c1/p1.png", "v1/c1/p2.png"])
                self.assertEqual(book.read("v1/c1/p2.png"), b"b")


if __name__ == "__main__":
    unittest.main()
