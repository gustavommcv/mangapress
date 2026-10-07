"""Run the real installers offline against generated release archives."""

import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
REPO_URL = "https://github.com/gustavommcv/mangapress"
LATEST_API = "https://api.github.com/repos/gustavommcv/mangapress/releases/latest"
NOTICES = ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md", "DEPENDENCY-LICENSES.txt")
WINDOWS = os.name == "nt"


class InstallerCases:
    engine = None

    @classmethod
    def setUpClass(cls):
        cls.executable_name = "mangapress.exe" if WINDOWS else "mangapress"
        cls.binary = ROOT / "target/debug" / cls.executable_name
        if not cls.binary.is_file():
            raise RuntimeError("Build the CLI with cargo build --workspace --locked first")
        cls.shell = shutil.which(cls.engine)
        if cls.shell is None:
            raise RuntimeError(f"Required test interpreter is missing: {cls.engine}")
        cls.version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
        cls.tag = "v" + cls.version
        if WINDOWS:
            cls.target = "x86_64-pc-windows-msvc"
        elif sys.platform == "darwin":
            arch = "aarch64" if platform.machine() in ("arm64", "aarch64") else "x86_64"
            cls.target = arch + "-apple-darwin"
        else:
            cls.target = "x86_64-unknown-linux-musl"
        cls.asset_name = f"mangapress-{cls.target}" + (".zip" if WINDOWS else ".tar.gz")

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="mangapress-installer-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.install_dir = self.directory / "install space ação [books]"
        self.stage_root = self.directory / "temporary"
        self.stage_root.mkdir()
        self.notices = {name: f"Synthetic notice for {name}\n".encode() for name in NOTICES}
        self.archive = self.directory / self.asset_name
        self.make_archive()
        self.checksums = self.directory / "checksums.txt"
        self.set_checksum()
        self.base_url = f"{REPO_URL}/releases/download/{self.tag}"
        self.log = self.directory / "downloads.log"
        self.catalog_file = self.directory / "catalog.json"
        self.catalog = {
            "log": str(self.log),
            "latest_web": REPO_URL + "/releases/latest",
            "latest_api": LATEST_API,
            "latest_tag": self.tag,
            "latest_redirect": REPO_URL + "/releases/tag/" + self.tag,
            "fail_urls": [],
            "files": {
                self.base_url + "/checksums.txt": str(self.checksums),
                self.base_url + "/" + self.asset_name: str(self.archive),
            },
        }
        self.environment = dict(os.environ)
        self.environment.pop("MANGAPRESS_VERSION", None)
        self.environment.update({
            "MANGAPRESS_INSTALL_DIR": str(self.install_dir),
            "MANGAPRESS_NO_PATH_UPDATE": "1",
            "INSTALLER_CATALOG": str(self.catalog_file),
            "TMPDIR": str(self.stage_root),
            "TEMP": str(self.stage_root),
            "TMP": str(self.stage_root),
        })
        if WINDOWS:
            # Each engine must discover its own built-in modules, not its parent's.
            self.environment = {key: value for key, value in self.environment.items()
                                if key.lower() != "psmodulepath"}
            self.environment["INSTALLER_SCRIPT"] = str(ROOT / "install.ps1")
            self.environment["INSTALLER_WRAPPER"] = str(ROOT / "tools/installers/run_windows_fixture.ps1")
            self.command = [self.shell, "-NoProfile", "-NonInteractive", "-Command",
                            "Invoke-Expression ([IO.File]::ReadAllText($env:INSTALLER_WRAPPER))"]
        else:
            self.mock_bin = self.directory / "mock-bin"
            self.mock_bin.mkdir()
            curl = self.mock_bin / "curl"
            curl.write_text(
                f"#!/bin/sh\nexec {shlex.quote(sys.executable)} "
                f"{shlex.quote(str(ROOT / 'tools/installers/download_fixture.py'))} \"$@\"\n",
                encoding="utf-8",
            )
            curl.chmod(0o755)
            self.environment["PATH"] = str(self.mock_bin) + os.pathsep + self.environment["PATH"]
            self.command = [self.shell, str(ROOT / "install.sh")]

    @property
    def license_dir(self):
        return self.install_dir if WINDOWS else self.install_dir / "mangapress-licenses"

    def make_archive(self, include_binary=True):
        files = dict(self.notices)
        if include_binary:
            files[self.executable_name] = self.binary.read_bytes()
        if WINDOWS:
            with zipfile.ZipFile(self.archive, "w", zipfile.ZIP_DEFLATED) as archive:
                for name, data in files.items():
                    archive.writestr(name, data)
        else:
            with tarfile.open(self.archive, "w:gz") as archive:
                for name, data in files.items():
                    entry = tarfile.TarInfo(name)
                    entry.size = len(data)
                    entry.mode = 0o755 if name == self.executable_name else 0o644
                    archive.addfile(entry, io.BytesIO(data))

    def set_checksum(self, marker=" ", ending="\n"):
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.checksums.write_bytes(f"{self.digest} {marker}{self.asset_name}{ending}".encode())

    def run_installer(self):
        self.catalog_file.write_text(json.dumps(self.catalog), encoding="utf-8")
        result = subprocess.run(self.command, env=self.environment, cwd=ROOT,
                                capture_output=True, text=True, encoding="utf-8", errors="replace")
        self.assertEqual(list(self.stage_root.iterdir()), [], result.stdout + result.stderr)
        return result

    def requests(self):
        return self.log.read_text(encoding="utf-8-sig").splitlines() if self.log.exists() else []

    def assert_installed(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual((self.install_dir / self.executable_name).read_bytes(), self.binary.read_bytes())
        for name, content in self.notices.items():
            self.assertEqual((self.license_dir / name).read_bytes(), content)
        self.assertIn("mangapress " + self.version, result.stdout)

    def assert_failed_without_replacing(self, message=None):
        self.install_dir.mkdir(exist_ok=True)
        self.license_dir.mkdir(exist_ok=True)
        binary = self.install_dir / self.executable_name
        notice = self.license_dir / "LICENSE-MIT"
        binary.write_bytes(b"existing executable")
        notice.write_bytes(b"existing license")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(binary.read_bytes(), b"existing executable")
        self.assertEqual(notice.read_bytes(), b"existing license")
        if message:
            self.assertIn(message.lower(), (result.stdout + result.stderr).lower())

    def test_latest_is_resolved_once_and_both_downloads_use_its_tag(self):
        self.assert_installed(self.run_installer())
        latest = LATEST_API if WINDOWS else REPO_URL + "/releases/latest"
        self.assertEqual(self.requests(), [latest, self.base_url + "/checksums.txt",
                                          self.base_url + "/" + self.asset_name])

    def test_specific_version_bypasses_latest(self):
        self.environment["MANGAPRESS_VERSION"] = self.tag
        self.catalog["latest_tag"] = "v99.0.0"
        self.catalog["latest_redirect"] = REPO_URL + "/releases/tag/v99.0.0"
        self.assert_installed(self.run_installer())
        self.assertEqual(self.requests(), [self.base_url + "/checksums.txt",
                                          self.base_url + "/" + self.asset_name])

    def test_version_without_prefix_is_normalized(self):
        self.environment["MANGAPRESS_VERSION"] = self.version
        self.assert_installed(self.run_installer())
        self.assertEqual(len(self.requests()), 2)

    def test_invalid_version_is_rejected_before_downloads(self):
        for version in ("../main", "--help", "v1.2.3/other", "v1.2.3\nother", "V1.2.3", "v1.2", " "):
            with self.subTest(version=version):
                self.environment["MANGAPRESS_VERSION"] = version
                self.assert_failed_without_replacing("invalid release version")
                self.assertEqual(self.requests(), [])

    def test_latest_failure_does_not_touch_an_installation(self):
        self.catalog["fail_urls"] = [LATEST_API if WINDOWS else REPO_URL + "/releases/latest"]
        self.assert_failed_without_replacing("download failed")

    def test_invalid_latest_tag_is_rejected(self):
        self.catalog["latest_tag"] = "../main"
        self.catalog["latest_redirect"] = REPO_URL + "/releases/tag/../main"
        self.assert_failed_without_replacing("invalid release version")

    def test_missing_selected_checksum_is_rejected(self):
        self.checksums.write_text("0" * 64 + "  another-platform.zip\n", encoding="utf-8")
        self.assert_failed_without_replacing("checksum")

    def test_failed_verification_does_not_create_an_install_folder(self):
        self.checksums.write_bytes(b"")
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.install_dir.exists())

    def test_duplicate_selected_checksum_is_rejected(self):
        self.checksums.write_bytes(self.checksums.read_bytes() * 2)
        self.assert_failed_without_replacing("duplicate")

    def test_malformed_selected_checksum_is_rejected(self):
        self.checksums.write_text("0" * 63 + f"  {self.asset_name}\n", encoding="utf-8")
        self.assert_failed_without_replacing("malformed")

    def test_trailing_checksum_fields_are_rejected(self):
        self.checksums.write_text(f"{self.digest}  {self.asset_name} extra\n", encoding="utf-8")
        self.assert_failed_without_replacing("malformed")

    def test_tampering_is_rejected_before_extraction(self):
        self.archive.write_bytes(b"not even an archive")
        self.assert_failed_without_replacing("checksum verification failed")

    def test_checksum_download_failure_preserves_existing_files(self):
        self.catalog["fail_urls"] = [self.base_url + "/checksums.txt"]
        self.assert_failed_without_replacing("download failed")

    def test_archive_download_failure_preserves_existing_files(self):
        self.catalog["fail_urls"] = [self.base_url + "/" + self.asset_name]
        self.assert_failed_without_replacing("download failed")

    def test_archive_without_binary_is_rejected(self):
        self.make_archive(include_binary=False)
        self.set_checksum()
        self.assert_failed_without_replacing()

    def test_binary_version_must_match_the_selected_tag(self):
        tag = "v99.0.0"
        base = f"{REPO_URL}/releases/download/{tag}"
        self.environment["MANGAPRESS_VERSION"] = tag
        self.catalog["files"] = {base + "/checksums.txt": str(self.checksums),
                                 base + "/" + self.asset_name: str(self.archive)}
        self.assert_failed_without_replacing("does not match")

    def test_binary_marker_crlf_and_other_platform_entries_are_supported(self):
        self.set_checksum(marker="*", ending="\r\n")
        with self.checksums.open("ab") as checksums:
            checksums.write(b"0" * 64 + b"  another-platform.zip\r\n")
        self.assert_installed(self.run_installer())

    def test_update_replaces_known_files_but_preserves_unrelated_files(self):
        self.install_dir.mkdir()
        unrelated = self.install_dir / "personal.txt"
        unrelated.write_bytes(b"keep me")
        (self.install_dir / self.executable_name).write_bytes(b"old executable")
        self.assert_installed(self.run_installer())
        self.assertEqual(unrelated.read_bytes(), b"keep me")

    def test_legacy_archive_does_not_leave_stale_dependency_notices(self):
        self.install_dir.mkdir()
        self.license_dir.mkdir(exist_ok=True)
        stale = self.license_dir / "DEPENDENCY-LICENSES.txt"
        stale.write_bytes(b"notices from another version")
        del self.notices["DEPENDENCY-LICENSES.txt"]
        self.make_archive()
        self.set_checksum()
        self.assert_installed(self.run_installer())
        self.assertFalse(stale.exists())


@unittest.skipIf(WINDOWS, "POSIX installer is tested on Linux and macOS")
class PosixInstallerTests(InstallerCases, unittest.TestCase):
    engine = "sh"

    def restrict_path(self, include_shasum):
        commands = ["uname", "mktemp", "grep", "awk", "tar", "chmod", "mkdir", "install", "rm"]
        if include_shasum:
            commands.append("shasum")
        for name in commands:
            source = shutil.which(name)
            if source is None:
                raise RuntimeError(f"Required native installer command is missing: {name}")
            (self.mock_bin / name).symlink_to(source)
        self.environment["PATH"] = str(self.mock_bin)

    def test_native_shasum_fallback_verifies_and_installs(self):
        self.restrict_path(include_shasum=True)
        self.assert_installed(self.run_installer())

    def test_missing_hash_tools_preserves_existing_files(self):
        self.restrict_path(include_shasum=False)
        self.assert_failed_without_replacing("install sha256sum or shasum")


@unittest.skipUnless(WINDOWS, "Windows installer requires Windows")
class WindowsPowerShellInstallerTests(InstallerCases, unittest.TestCase):
    engine = "powershell"


@unittest.skipUnless(WINDOWS, "Windows installer requires Windows")
class PowerShell7InstallerTests(InstallerCases, unittest.TestCase):
    engine = "pwsh"


if __name__ == "__main__":
    unittest.main()
