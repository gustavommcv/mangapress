"""Refuse a release tag that does not match the workspace package version."""

import argparse
from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[2]


def check_tag(tag, manifest):
    version = tomllib.loads(manifest.read_text(encoding="utf-8"))["workspace"][
        "package"
    ]["version"]
    expected = f"v{version}"
    if tag != expected:
        raise ValueError(
            f"tag {tag!r} does not match the package version; expected {expected}"
        )
    return version


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", help="existing v-prefixed release tag")
    args = parser.parse_args()
    try:
        version = check_tag(args.tag, ROOT / "Cargo.toml")
    except (OSError, ValueError, KeyError) as error:
        parser.exit(1, f"Release tag validation failed: {error}\n")
    print(f"Release tag matches mangapress {version}")


if __name__ == "__main__":
    main()
