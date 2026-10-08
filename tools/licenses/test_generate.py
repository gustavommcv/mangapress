"""Regression tests for omissions observed in generated license notices."""

import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from generate import CARGO_ABOUT_VERSION, generate, validate_report


class NoticeValidationTests(unittest.TestCase):
    def setUp(self):
        self.original = "Copyright (c) Example & Contributors\nPermission notice.\n"
        self.report = {
            "licenses": [{
                "id": "MIT",
                "text": self.original,
                "source_path": "LICENSE-MIT",
                "used_by": [{"crate": {
                    "name": "example",
                    "version": "1.2.3",
                    "source": "registry+https://github.com/rust-lang/crates.io-index",
                }}],
            }],
        }
        self.rendered = f"- example 1.2.3\n{self.original}"

    def test_original_text_and_attribution_are_accepted(self):
        validate_report(self.report, self.rendered)

    def test_windows_newlines_preserve_the_notice(self):
        validate_report(self.report, self.rendered.replace("\n", "\r\n"))

    def test_generic_fallback_cannot_replace_dependency_copyright(self):
        self.report["licenses"][0]["source_path"] = None
        with self.assertRaisesRegex(ValueError, "generic MIT fallback"):
            validate_report(self.report, self.rendered)

    def test_only_own_workspace_crates_can_use_separately_shipped_licenses(self):
        for name in ("mangapress-cli", "mangapress-core"):
            with self.subTest(name=name):
                report = copy.deepcopy(self.report)
                entry = report["licenses"][0]
                entry["source_path"] = None
                entry["used_by"][0]["crate"].update(name=name, source=None)
                validate_report(report, f"- {name} 1.2.3\n{self.original}")

    def test_unreviewed_path_dependency_still_requires_a_source_notice(self):
        entry = self.report["licenses"][0]
        entry["source_path"] = None
        entry["used_by"][0]["crate"]["source"] = None
        with self.assertRaisesRegex(ValueError, "generic MIT fallback"):
            validate_report(self.report, self.rendered)

    def test_registry_crate_cannot_impersonate_a_workspace_crate(self):
        entry = self.report["licenses"][0]
        entry["source_path"] = None
        entry["used_by"][0]["crate"]["name"] = "mangapress-core"
        with self.assertRaisesRegex(ValueError, "generic MIT fallback"):
            validate_report(self.report, f"- mangapress-core 1.2.3\n{self.original}")

    def test_escaped_or_truncated_text_is_rejected(self):
        for rendered in (
            self.rendered.replace("&", "&amp;"),
            "- example 1.2.3\nPermission notice.\n",
        ):
            with self.subTest(rendered=rendered):
                with self.assertRaisesRegex(ValueError, "original license text missing"):
                    validate_report(self.report, rendered)

    def test_missing_dependency_attribution_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "missing attribution for example"):
            validate_report(self.report, self.original)

    def test_another_version_cannot_satisfy_the_dependency_attribution(self):
        with self.assertRaisesRegex(ValueError, "missing attribution for example"):
            validate_report(self.report, self.rendered.replace("1.2.3", "1.2.30"))

    def test_empty_graph_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "no license texts"):
            validate_report({"licenses": []}, "")

    def test_empty_text_or_attribution_is_rejected(self):
        for field, value in (("text", "\n"), ("used_by", [])):
            with self.subTest(field=field):
                report = copy.deepcopy(self.report)
                report["licenses"][0][field] = value
                with self.assertRaisesRegex(ValueError, "missing text or dependency"):
                    validate_report(report, self.rendered)

    def run_generator(self, root, report=None, error=None):
        def run(command, **kwargs):
            if "--version" in command:
                return subprocess.CompletedProcess(
                    command, 0, stdout=f"cargo-about {CARGO_ABOUT_VERSION}\n"
                )
            if error:
                raise error
            self.assertIn("--locked", command)
            self.assertIn("--fail", command)
            self.assertIn("x86_64-pc-windows-msvc", command)
            destination = Path(command[command.index("--output-file") + 1])
            contents = (
                json.dumps(self.report if report is None else report)
                if "--format" in command else self.rendered
            )
            destination.write_text(contents, encoding="utf-8")
            return subprocess.CompletedProcess(command, 0)

        with (
            patch("generate.ROOT", root),
            patch("generate.subprocess.run", side_effect=run),
        ):
            return generate("x86_64-pc-windows-msvc", "cargo-about")

    def test_only_verified_plain_text_is_published(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = self.run_generator(Path(temporary))
            self.assertEqual(output.name, "DEPENDENCY-LICENSES.txt")
            self.assertEqual(output.read_text(encoding="utf-8"), self.rendered)
            self.assertEqual(list(output.parent.iterdir()), [output])

    def test_failed_validation_cannot_publish_partial_notices(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "target/dependency-notices/x86_64-pc-windows-msvc"
            directory.mkdir(parents=True)
            output = directory / "DEPENDENCY-LICENSES.txt"
            output.write_text("previous verified report", encoding="utf-8")
            report = copy.deepcopy(self.report)
            report["licenses"][0]["source_path"] = None
            with self.assertRaisesRegex(ValueError, "generic MIT fallback"):
                self.run_generator(root, report=report)
            self.assertEqual(
                output.read_text(encoding="utf-8"), "previous verified report"
            )
            self.assertEqual(list(directory.iterdir()), [output])

    def test_tool_failure_leaves_no_distributable_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(subprocess.CalledProcessError):
                self.run_generator(
                    root, error=subprocess.CalledProcessError(1, "cargo-about")
                )
            directory = root / "target/dependency-notices/x86_64-pc-windows-msvc"
            self.assertEqual(list(directory.iterdir()), [])

    def test_unpinned_tool_version_is_rejected_before_generation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with (
                patch("generate.ROOT", root),
                patch("generate.subprocess.run") as run,
            ):
                run.return_value.stdout = "cargo-about 0.0.0\n"
                with self.assertRaisesRegex(
                    ValueError, f"expected cargo-about {CARGO_ABOUT_VERSION}"
                ):
                    generate("x86_64-pc-windows-msvc", "cargo-about")
                self.assertEqual(run.call_count, 1)
                self.assertEqual(list(root.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
