"""Render cargo-about's locked dependency report without losing source notices."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
CARGO_ABOUT_VERSION = "0.9.2"
WORKSPACE_CRATES = {"mangapress-cli", "mangapress-core"}


def validate_report(report, rendered):
    """Check provenance and rendering; cargo-about resolves the license graph."""
    licenses = report.get("licenses", [])
    if not licenses:
        raise ValueError("cargo-about returned no license texts")
    rendered = rendered.replace("\r\n", "\n")
    rendered_lines = rendered.splitlines()
    for license_entry in licenses:
        license_id = license_entry["id"]
        original = license_entry["text"].replace("\r\n", "\n")
        used_by = license_entry["used_by"]
        if not original.strip() or not used_by:
            raise ValueError(f"{license_id}: missing text or dependency attribution")
        if original not in rendered:
            raise ValueError(f"{license_id}: original license text missing from output")
        for dependency in used_by:
            crate = dependency["crate"]
            attribution = f"- {crate['name']} {crate['version']}"
            if not any(
                line == attribution or line.startswith(attribution + " (")
                for line in rendered_lines
            ):
                raise ValueError(f"{license_id}: missing attribution for {crate['name']}")
            is_workspace = (
                crate.get("source") is None and crate["name"] in WORKSPACE_CRATES
            )
            if not is_workspace and not license_entry.get("source_path"):
                raise ValueError(
                    f"{crate['name']}: generic {license_id} fallback has no source "
                    "notice; review about.toml instead of dropping its copyright"
                )


def generate(target, cargo_about):
    version = subprocess.run(
        [cargo_about, "--version"], check=True, capture_output=True, text=True
    ).stdout.strip()
    if version != f"cargo-about {CARGO_ABOUT_VERSION}":
        raise ValueError(f"expected cargo-about {CARGO_ABOUT_VERSION}, found {version}")
    output_dir = ROOT / "target" / "dependency-notices" / target
    output_dir.mkdir(parents=True, exist_ok=True)
    common = [
        cargo_about,
        "generate",
        "--locked",
        "--fail",
        "--manifest-path",
        str(ROOT / "crates/mangapress-cli/Cargo.toml"),
        "--target",
        target,
    ]
    # Neither local source paths in the JSON nor an unvalidated report are shipped.
    with tempfile.TemporaryDirectory(dir=output_dir) as temporary:
        report_path = Path(temporary) / "report.json"
        notice_path = Path(temporary) / "DEPENDENCY-LICENSES.txt"
        subprocess.run(
            common + ["--format", "json", "--output-file", str(report_path)],
            check=True, cwd=ROOT,
        )
        subprocess.run(
            common + [
                str(ROOT / "tools/licenses/notices.hbs"),
                "--output-file",
                str(notice_path),
            ],
            check=True,
            cwd=ROOT,
        )
        validate_report(
            json.loads(report_path.read_text(encoding="utf-8")),
            notice_path.read_text(encoding="utf-8"),
        )
        output = output_dir / notice_path.name
        os.replace(notice_path, output)
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, help="release target triple")
    parser.add_argument(
        "--cargo-about", default="cargo-about", help="cargo-about executable"
    )
    args = parser.parse_args()
    if not re.fullmatch(r"[a-zA-Z0-9_-]+", args.target):
        parser.error("target must be a Rust target triple, not a file path")
    try:
        output = generate(args.target, args.cargo_about)
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Dependency notices could not be generated: {error}\n")
    print(f"Verified dependency notices: {output.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
