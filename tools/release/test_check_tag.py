"""A v* trigger alone must not publish a mislabeled executable."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from check_tag import ROOT, check_tag


class ReleaseTagTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.manifest = Path(self.temporary.name) / "Cargo.toml"
        self.manifest.write_text(
            '[workspace.package]\nversion = "1.2.3"\n', encoding="utf-8"
        )

    def test_matching_version_is_accepted(self):
        self.assertEqual(check_tag("v1.2.3", self.manifest), "1.2.3")

    def test_wrong_versions_prefixes_and_suffixes_are_rejected(self):
        for tag in (
            "v1.2.4", "1.2.3", "V1.2.3", "v1.2.3-rc.1", "v1.2.3 ", "vnext", ""
        ):
            with self.subTest(tag=tag):
                with self.assertRaisesRegex(ValueError, "expected v1.2.3"):
                    check_tag(tag, self.manifest)

    def test_a_prerelease_requires_its_exact_manifest_version(self):
        self.manifest.write_text(
            '[workspace.package]\nversion = "1.2.3-rc.1"\n', encoding="utf-8"
        )
        self.assertEqual(check_tag("v1.2.3-rc.1", self.manifest), "1.2.3-rc.1")
        with self.assertRaises(ValueError):
            check_tag("v1.2.3", self.manifest)

    def test_missing_package_version_is_not_accepted(self):
        self.manifest.write_text("[workspace]\n", encoding="utf-8")
        with self.assertRaises(KeyError):
            check_tag("v1.2.3", self.manifest)

    def test_actual_command_reports_failure_without_a_traceback(self):
        result = subprocess.run(
            [sys.executable, str(ROOT / "tools/release/check_tag.py"),
             "v-not-a-version"],
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("Release tag validation failed:", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
