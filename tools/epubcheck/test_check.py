"""Failure controls for the EPUB gate; the workflow also runs the real checker."""

import copy
from contextlib import redirect_stdout
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import check


class GateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.enterContext(redirect_stdout(io.StringIO()))
        self.directory = Path(self.temp.name).resolve()
        self.book = self.directory / "book.epub"
        self.book.write_bytes(b"synthetic book placeholder")
        self.path = self.book.with_suffix(".json")
        self.report = {"checker": {"path": str(self.book), "checkerVersion": check.VERSION,
                                   "nFatal": 0, "nError": 0, "nWarning": 0}, "messages": [],
                       "publication": {"nSpines": 1}}

    def write_report(self, report=None):
        self.path.write_text(json.dumps(self.report if report is None else report), encoding="utf-8")

    def test_clean_report_and_informative_messages_pass(self):
        self.report["messages"] = [{"ID": "INF-001", "severity": "INFO"}]
        self.write_report()
        check.require_clean(0, check.read_report(self.path, self.book))

    def test_missing_and_malformed_reports_fail(self):
        with self.assertRaises(OSError):
            check.read_report(self.path, self.book)
        self.path.write_text("not JSON", encoding="utf-8")
        with self.assertRaises(ValueError):
            check.read_report(self.path, self.book)
        for report in ([], {}, {"checker": []}):
            self.write_report(report)
            with self.subTest(report=report), self.assertRaisesRegex(ValueError, "metadata"):
                check.read_report(self.path, self.book)

    def test_wrong_version_or_unrelated_report_fails(self):
        for key, value in (("checkerVersion", "5.3.0"), ("path", str(self.directory / "old.epub")),
                           ("path", None)):
            report = copy.deepcopy(self.report)
            report["checker"][key] = value
            self.write_report(report)
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                check.read_report(self.path, self.book)

    def test_every_failure_counter_is_required_and_nonnegative_integer(self):
        for key in check.COUNTERS:
            for value in (None, -1, True, "0"):
                report = copy.deepcopy(self.report)
                report["checker"][key] = value
                self.write_report(report)
                with self.subTest(key=key, value=value), self.assertRaisesRegex(ValueError, "counter"):
                    check.read_report(self.path, self.book)

    def test_messages_cannot_be_missing_or_silently_discarded(self):
        for messages in (None, {}, [None], [{}], [{"ID": "RSC-001", "severity": "unknown"}]):
            report = dict(self.report, messages=messages)
            self.write_report(report)
            with self.subTest(messages=messages), self.assertRaisesRegex(ValueError, "messages"):
                check.read_report(self.path, self.book)

    def test_metadata_only_or_empty_publication_is_not_a_checked_book(self):
        for publication in (None, {}, {"nSpines": 0}, {"nSpines": True}, {"nSpines": "4"}):
            self.write_report(dict(self.report, publication=publication))
            with self.subTest(publication=publication), self.assertRaisesRegex(ValueError, "spine"):
                check.read_report(self.path, self.book)

    def test_nonzero_exit_fails_even_with_zero_counters(self):
        with self.assertRaisesRegex(ValueError, "exit=2"):
            check.require_clean(2, self.report)

    def test_fatal_error_and_warning_counters_fail_even_with_zero_exit(self):
        for key in check.COUNTERS:
            report = copy.deepcopy(self.report)
            report["checker"][key] = 1
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "rejected"):
                check.require_clean(0, report)

    def test_failure_messages_cannot_hide_behind_zero_counters(self):
        for severity in check.FAILURE_SEVERITIES:
            report = dict(self.report, messages=[{"ID": "TEST-001", "severity": severity}])
            with self.subTest(severity=severity), self.assertRaisesRegex(ValueError, "TEST-001"):
                check.require_clean(0, report)

    def test_missing_empty_book_and_stale_report_fail_before_running_java(self):
        with patch.object(check.subprocess, "run") as run:
            self.book.unlink()
            with self.assertRaisesRegex(ValueError, "nonempty"):
                check.check_book(Path("checker.jar"), self.book, "java")
            self.book.touch()
            with self.assertRaisesRegex(ValueError, "nonempty"):
                check.check_book(Path("checker.jar"), self.book, "java")
            self.book.write_bytes(b"book")
            self.write_report()
            with self.assertRaisesRegex(ValueError, "stale"):
                check.check_book(Path("checker.jar"), self.book, "java")
            run.assert_not_called()

    def test_checker_command_is_strict_and_saves_diagnostics(self):
        def execute(command, **kwargs):
            self.write_report()
            return subprocess.CompletedProcess(command, 0, "checker output", "checker diagnostics")
        with patch.object(check.subprocess, "run", side_effect=execute) as run:
            check.require_clean(*check.check_book(Path("checker.jar"), self.book, "my-java"))
        self.assertEqual(run.call_args.args[0], [
            "my-java", "-jar", "checker.jar", str(self.book), "--locale", "en", "--failonwarnings",
            "--maxOfEachMessage", "unlimited", "--json", str(self.path)])
        self.assertIn("checker diagnostics", self.book.with_suffix(".log").read_text(encoding="utf-8"))

    def test_no_report_from_a_successful_process_is_not_a_pass(self):
        with patch.object(check.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")):
            with self.assertRaises(OSError):
                check.check_book(Path("checker.jar"), self.book, "java")

    def test_empty_or_duplicate_case_registry_fails(self):
        for cases in ((), (check.Case("same"), check.Case("same"))):
            with self.subTest(cases=cases), self.assertRaisesRegex(ValueError, "nonempty and uniquely"):
                check.run_suite(Path("cli"), Path("jar"), self.directory, "java", cases)

    def test_successful_cli_without_output_cannot_use_a_stale_book_elsewhere(self):
        # The pre-existing self.book is not a product of this case, and is never globbed.
        with patch.object(check.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "", "")):
            with self.assertRaisesRegex(ValueError, "failed"):
                check.run_suite(Path("cli"), Path("jar"), self.directory, "java", (check.Case("fresh"),))
        summary = json.loads((self.directory / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual([item["status"] for item in summary], ["failed", "failed"])
        self.assertIn("nonempty book", summary[0]["reason"])
        self.assertTrue(self.book.is_file())

    def test_missing_cli_or_java_is_a_failure_not_a_skipped_case(self):
        for produced in (False, True):
            def execute(command, **kwargs):
                if produced and "--output" in command:
                    Path(command[command.index("--output") + 1]).write_bytes(b"book")
                    return subprocess.CompletedProcess(command, 0, "", "")
                raise FileNotFoundError("executable is not installed")
            directory = self.directory / str(produced)
            directory.mkdir()
            with patch.object(check.subprocess, "run", side_effect=execute):
                with self.subTest(produced=produced), self.assertRaisesRegex(ValueError, "failed"):
                    check.run_suite(Path("cli"), Path("jar"), directory, "java", (check.Case("test"),))

    def test_a_failed_case_is_not_masked_by_a_valid_book_and_negative_control(self):
        def convert(command, **kwargs):
            output = Path(command[command.index("--output") + 1])
            output.write_bytes(b"book")
            return subprocess.CompletedProcess(command, 2 if output.parent.name == "failed" else 0, "", "")
        rejected = copy.deepcopy(self.report)
        rejected["checker"]["nError"] = 1
        rejected["messages"] = [{"ID": "RSC-001", "severity": "ERROR"}]
        with patch.object(check.subprocess, "run", side_effect=convert), \
             patch.object(check, "broken_resource_copy"), \
             patch.object(check, "check_book", side_effect=((0, self.report), (1, rejected))) as validate:
            with self.assertRaisesRegex(ValueError, "failed"):
                check.run_suite(Path("cli"), Path("jar"), self.directory, "java",
                                (check.Case("passed"), check.Case("failed")))
        self.assertEqual(validate.call_count, 2)  # Valid book and control, never the failed CLI output.
        summary = json.loads((self.directory / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual([item["status"] for item in summary], ["passed", "failed", "passed"])

    def test_negative_control_requires_nonzero_exit_and_the_missing_resource_error(self):
        def convert(command, **kwargs):
            Path(command[command.index("--output") + 1]).write_bytes(b"book")
            return subprocess.CompletedProcess(command, 0, "", "")
        rejected = copy.deepcopy(self.report)
        rejected["checker"]["nError"] = 1
        rejected["messages"] = [{"ID": "RSC-001", "severity": "ERROR"}]
        wrong_error = copy.deepcopy(rejected)
        wrong_error["messages"][0]["ID"] = "OPF-027"
        mixed = copy.deepcopy(rejected)
        mixed["messages"] += [{"ID": "OPF-027", "severity": "ERROR"}]
        fatal = copy.deepcopy(rejected)
        fatal["checker"]["nFatal"] = 1
        outcomes = ((0, self.report), (0, rejected), (1, wrong_error), (1, mixed), (1, fatal), (1, rejected))
        for index, outcome in enumerate(outcomes):
            directory = self.directory / str(index)
            directory.mkdir()
            with patch.object(check.subprocess, "run", side_effect=convert), \
                 patch.object(check, "broken_resource_copy"), \
                 patch.object(check, "check_book", side_effect=((0, self.report), outcome)):
                with self.subTest(outcome=outcome):
                    if index == len(outcomes) - 1:
                        self.assertEqual(check.run_suite(Path("cli"), Path("jar"), directory,
                                                         "java", (check.Case("test"),)), 1)
                    else:
                        with self.assertRaisesRegex(ValueError, "failed"):
                            check.run_suite(Path("cli"), Path("jar"), directory, "java", (check.Case("test"),))

    def test_broken_copy_removes_only_a_page_image_and_preserves_zip_properties(self):
        members = {
            "mimetype": b"application/epub+zip",
            "META-INF/container.xml": b'<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
                                      b'<rootfiles><rootfile full-path="OEBPS/book.opf"/></rootfiles></container>',
            "OEBPS/book.opf": b'<package xmlns="http://www.idpf.org/2007/opf"><manifest>'
                              b'<item href="cover.jpg" media-type="image/jpeg" properties="cover-image"/>'
                              b'<item href="page.png" media-type="image/png"/></manifest></package>',
            "OEBPS/cover.jpg": b"cover",
            "OEBPS/page.png": b"page",
        }
        with zipfile.ZipFile(self.book, "w") as archive:
            for name, content in members.items():
                archive.writestr(name, content, compress_type=zipfile.ZIP_STORED if name == "mimetype"
                                 else zipfile.ZIP_DEFLATED)
        broken = self.directory / "broken.epub"
        check.broken_resource_copy(self.book, broken)
        with zipfile.ZipFile(self.book) as original, zipfile.ZipFile(broken) as output:
            self.assertEqual(set(output.namelist()), set(members) - {"OEBPS/page.png"})
            self.assertIn("OEBPS/page.png", original.namelist())
            for name in output.namelist():
                self.assertEqual(output.read(name), original.read(name))
                self.assertEqual(output.getinfo(name).compress_type, original.getinfo(name).compress_type)


class DistributionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.archive = self.directory / "tool.zip"

    def package(self, entries):
        with zipfile.ZipFile(self.archive, "w") as package:
            for name in entries:
                package.writestr(f"epubcheck-{check.VERSION}/{name}", b"test payload")
        return hashlib.sha256(self.archive.read_bytes()).hexdigest()

    def test_checksum_mismatch_prevents_extraction(self):
        self.package(("epubcheck.jar", "lib/dependency.jar"))
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            check.prepare_tool(self.directory, self.archive)
        self.assertFalse((self.directory / "tool").exists())

    def test_verified_distribution_keeps_libraries_and_license(self):
        digest = self.package(("epubcheck.jar", "lib/dependency.jar", "LICENSE.txt"))
        with patch.object(check, "SHA256", digest):
            jar = check.prepare_tool(self.directory, self.archive)
        self.assertTrue(jar.is_file())
        self.assertTrue((jar.parent / "lib/dependency.jar").is_file())
        self.assertTrue((jar.parent / "LICENSE.txt").is_file())

    def test_jar_or_library_omissions_fail(self):
        for index, entries in enumerate((("epubcheck.jar",), ("lib/dependency.jar",))):
            digest = self.package(entries)
            directory = self.directory / str(index)
            directory.mkdir()
            with patch.object(check, "SHA256", digest), self.subTest(entries=entries):
                with self.assertRaisesRegex(ValueError, "JAR or libraries"):
                    check.prepare_tool(directory, self.archive)


if __name__ == "__main__":
    unittest.main()
