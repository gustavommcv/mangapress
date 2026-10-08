"""Generate fresh CLI books and validate them with the pinned official EPUBCheck."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from urllib.request import urlopen
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from cases import CASES, Case, prepare_case, snapshot, verify_bmp_rejection, verify_book, verify_events

VERSION = "5.4.0"
URL = f"https://github.com/w3c/epubcheck/releases/download/v{VERSION}/epubcheck-{VERSION}.zip"
SHA256 = "33350c61038e71dfb3d45a76aed04bf5481e6d5500cb780f6e98db8bbd15a28c"
COUNTERS = ("nFatal", "nError", "nWarning")
FAILURE_SEVERITIES = {"FATAL", "ERROR", "WARNING"}


def prepare_tool(directory, archive=None):
    """Verify the official ZIP before extracting the JAR, libraries and licenses."""
    if archive is None:
        archive = directory / "epubcheck.zip"
        with urlopen(URL, timeout=60) as response, archive.open("wb") as output:
            shutil.copyfileobj(response, output)
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    if digest != SHA256:
        raise ValueError(f"EPUBCheck ZIP checksum mismatch: expected {SHA256}, got {digest}")
    # Only the authenticated upstream distribution is extracted, never an input book.
    with zipfile.ZipFile(archive) as package:
        package.extractall(directory / "tool")
    distribution = directory / "tool" / f"epubcheck-{VERSION}"
    jar = distribution / "epubcheck.jar"
    if not jar.is_file() or not any((distribution / "lib").glob("*.jar")):
        raise ValueError("EPUBCheck distribution is missing its JAR or libraries")
    return jar


def read_report(path, book):
    """Reject missing, malformed, wrong-version or unrelated checker reports."""
    report = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(report, dict) or not isinstance(report.get("checker"), dict):
        raise ValueError("missing EPUBCheck report metadata")
    checker = report["checker"]
    if checker.get("checkerVersion") != VERSION:
        raise ValueError(f"expected EPUBCheck {VERSION}, got {checker.get('checkerVersion')}")
    if not isinstance(checker.get("path"), str) or Path(checker["path"]).resolve() != book.resolve():
        raise ValueError("EPUBCheck report belongs to a different book")
    for key in COUNTERS:
        if type(checker.get(key)) is not int or checker[key] < 0:
            raise ValueError(f"missing or invalid EPUBCheck counter: {key}")
    publication = report.get("publication")
    if not isinstance(publication, dict) or type(publication.get("nSpines")) is not int \
            or publication["nSpines"] <= 0:
        raise ValueError("EPUBCheck report contains no parsed book spine")
    messages = report.get("messages")
    if not isinstance(messages, list) or any(
        not isinstance(message, dict) or not isinstance(message.get("ID"), str)
        or message.get("severity") not in FAILURE_SEVERITIES | {"INFO", "USAGE"}
        for message in messages
    ):
        raise ValueError("missing or malformed EPUBCheck messages")
    return report


def check_book(jar, book, java):
    if not book.is_file() or book.stat().st_size == 0:
        raise ValueError(f"CLI did not produce a nonempty book: {book}")
    report_path = book.with_suffix(".json")
    if report_path.exists():
        raise ValueError(f"refusing a stale report: {report_path}")
    result = subprocess.run(
        [java, "-jar", str(jar), str(book), "--locale", "en", "--failonwarnings",
         "--maxOfEachMessage", "unlimited", "--json", str(report_path)],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    book.with_suffix(".log").write_text(
        f"Exit status: {result.returncode}\n{result.stdout}\n{result.stderr}", encoding="utf-8"
    )
    return result.returncode, read_report(report_path, book)


def require_clean(status, report):
    counts = {key: report["checker"][key] for key in COUNTERS}
    failures = [message["ID"] for message in report["messages"]
                if message["severity"] in FAILURE_SEVERITIES]
    if status != 0 or any(counts.values()) or failures:
        raise ValueError(f"EPUBCheck rejected the book: exit={status}, {counts}, messages={failures}")


def require_missing_resource(status, report):
    counts = report["checker"]
    failures = [message for message in report["messages"]
                if message["severity"] in FAILURE_SEVERITIES]
    if status != 1 or counts["nError"] == 0 or counts["nFatal"] != 0 or counts["nWarning"] != 0 \
            or not failures or any(message["ID"] != "RSC-001" or message["severity"] != "ERROR"
                                   for message in failures):
        raise ValueError("EPUBCheck did not report only the deliberately missing resource")


def broken_resource_copy(source, destination):
    """Remove one declared page image, leaving its manifest and XHTML references."""
    ns = {"c": "urn:oasis:names:tc:opendocument:xmlns:container",
          "opf": "http://www.idpf.org/2007/opf"}
    with zipfile.ZipFile(source) as original, zipfile.ZipFile(destination, "w") as broken:
        container = ET.fromstring(original.read("META-INF/container.xml"))
        opf_path = container.find(".//c:rootfile", ns).attrib["full-path"]
        opf = ET.fromstring(original.read(opf_path))
        image = next(item for item in opf.findall("opf:manifest/opf:item", ns)
                     if item.attrib["media-type"].startswith("image/")
                     and "cover-image" not in item.attrib.get("properties", "").split())
        missing = (Path(opf_path).parent / image.attrib["href"]).as_posix()
        original.getinfo(missing)
        for entry in original.infolist():
            if entry.filename != missing:
                broken.writestr(entry, original.read(entry))


def run_suite(binary, jar, directory, java, cases=CASES):
    if not cases or len({case.name for case in cases}) != len(cases):
        raise ValueError("book cases must be nonempty and uniquely named")
    results, clean_books = [], []
    for case in cases:
        case_dir = directory / case.name
        case_dir.mkdir()
        book = case_dir / "book.epub"
        try:
            source, extra, originals, unchanged = prepare_case(case_dir, case)
            command = [str(binary), str(source), "--output", str(book), "--format", "epub",
                       "--profile", case.profile, "--cropping", "disabled", "--json-events", *case.flags, *extra]
            for dry_run in (True, False):
                log = "plan.log" if dry_run else "cli.log"
                result = subprocess.run(command + (["--dry-run"] if dry_run else []),
                                        capture_output=True, text=True, encoding="utf-8", errors="strict")
                (case_dir / log).write_text(
                    f"Exit status: {result.returncode}\n{result.stdout}\n{result.stderr}", encoding="utf-8"
                )
                rejected = not dry_run and case.reject_bmp
                if rejected:
                    verify_bmp_rejection(result.returncode, result.stdout, result.stderr, book)
                elif result.returncode != 0 or result.stderr:
                    raise ValueError(f"mangapress failed: exit={result.returncode}; inspect {log}")
                if dry_run and book.exists():
                    raise ValueError("dry run unexpectedly wrote a book")
                if not dry_run and not rejected and (not book.is_file() or book.stat().st_size == 0):
                    raise ValueError("CLI did not produce a nonempty book")
                if not rejected:
                    verify_events(result.stdout, case, book, dry_run)
                if snapshot(unchanged) != unchanged:
                    raise ValueError("CLI changed an input file")
            if not case.reject_bmp:
                verify_book(book, case, originals)
                require_clean(*check_book(jar, book, java))
        except (OSError, ValueError, KeyError, StopIteration, AttributeError, ET.ParseError, zipfile.BadZipFile) as error:
            results.append({"case": case.name, "status": "failed", "reason": str(error)})
            print(f"FAIL {case.name}: {error}", flush=True)
        else:
            if not case.reject_bmp:
                clean_books.append(book)
            results.append({"case": case.name, "status": "passed"})
            print(f"ok   {case.name}" + (" (expected CLI rejection)" if case.reject_bmp else ""), flush=True)
    try:
        if not clean_books:
            raise ValueError("no validated book is available for the negative control")
        broken = directory / "missing-resource.epub"
        broken_resource_copy(clean_books[0], broken)
        require_missing_resource(*check_book(jar, broken, java))
    except (OSError, ValueError, KeyError, StopIteration, ET.ParseError, zipfile.BadZipFile) as error:
        results.append({"case": "missing-resource-control", "status": "failed", "reason": str(error)})
        print(f"FAIL missing-resource-control: {error}", flush=True)
    else:
        results.append({"case": "missing-resource-control", "status": "passed"})
        print("ok   missing-resource-control (expected rejection)", flush=True)
    (directory / "summary.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    if any(result["status"] != "passed" for result in results):
        raise ValueError(f"EPUB conformance checks failed; inspect {directory}")
    return len(clean_books)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, help="offline copy of the pinned official EPUBCheck ZIP")
    parser.add_argument("--java", default="java", help="Java executable (not needed by mangapress itself)")
    parser.add_argument("--work", type=Path, default=ROOT / "target/epubcheck/books")
    args = parser.parse_args()
    args.work.mkdir(parents=True, exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix="run-", dir=args.work)).resolve()
    print(f"EPUBCheck {VERSION}: artifacts in {directory}", flush=True)
    try:
        jar = prepare_tool(directory, args.archive)
        subprocess.run(["cargo", "build", "--release", "--locked", "-p", "mangapress-cli"],
                       cwd=ROOT, check=True)
        binary = ROOT / "target/release" / ("mangapress.exe" if os.name == "nt" else "mangapress")
        checked = run_suite(binary, jar, directory, args.java)
    except (OSError, ValueError, subprocess.CalledProcessError, zipfile.BadZipFile) as error:
        parser.exit(1, f"EPUB conformance checks could not complete: {error}; artifacts: {directory}\n")
    print(f"EPUBCheck {VERSION}: {checked} fresh books passed; BMP passthrough refused for folder/CBZ; "
          "broken-resource control rejected")


if __name__ == "__main__":
    main()
