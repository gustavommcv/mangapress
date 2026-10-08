# Checking complete EPUBs

This development-only gate builds the CLI, generates small synthetic books, and runs the
official [EPUBCheck 5.4.0](https://github.com/w3c/epubcheck/releases/tag/v5.4.0) on each complete
archive. It is independent of KCC and supplements Rust tests and parity comparisons.
Java and Python are not mangapress runtime dependencies; `cargo test` still needs neither.

## Run locally

Use the repository's Rust toolchain, Python 3.11 or newer, and Java on `PATH`.
CI uses Python 3.13 and Eclipse Temurin 21. From the repository root:

```sh
python -m pip install -r tools/book-fixtures-requirements.txt
python -m unittest discover -s tools/epubcheck -p "test_*.py" -v
python tools/epubcheck/check.py
```

The command downloads the official ZIP and verifies its fixed SHA-256 before extracting
the JAR, adjacent libraries and license files. The version, URL and digest live together
in `check.py`. No unverified JAR override or automatic latest-version fallback is supported.
For an offline run, provide the same official ZIP; its digest is still checked:

```sh
python tools/epubcheck/check.py --archive /path/to/epubcheck-5.4.0.zip
```

`--java` accepts a Java executable path. `--work` changes the artifact root; the default
is ignored `target/epubcheck/books/`. Every run creates a fresh directory, with named cases
and exact expected output paths. Old files or an empty case list cannot satisfy the gate.
Books, CLI logs, checker JSON/text reports, and `summary.json` remain there for diagnosis.
A download, build, tool, output, report or validation failure exits nonzero.

## Initial coverage and failure policy

| Case | Generated output |
| --- | --- |
| `kindle-jpeg` | Kindle 11, profile-default grayscale JPEG, ComicInfo and flat chapter navigation |
| `kindle-centered-png` | Kindle 11 PNG with `--onepagelandscape` |
| `kobo-color-png` | Kobo Libra Colour with color RGB PNG |
| `custom-rotated-png` | Odd custom 127 × 193 target and a rotated double-page spread |

These use the same small chapter fixture as the actual CLI-to-book KCC checks, moved
unchanged to `tools/book_fixtures.py`. They do not compare KCC or assert pixel parity.
The checker receives complete EPUBs, not isolated OPF/XHTML files, with JSON output,
unlimited message occurrences, and `--failonwarnings`. Zero errors, fatal errors and
warnings are required; there is no ignore list or changed message severity.
The exit status and report counters/messages must agree on success. Missing, malformed,
wrong-version, unrelated or pre-existing reports cannot count as a pass.

A separate negative control removes one declared page image from a copy of a validated
book. The original remains intact. The checker must exit with failure and identify the
missing resource (`RSC-001`); an unrelated error or a tool crash is not accepted.
Unit tests exercise the gate's own failure handling, but do not substitute for this real run.

This first gate covers selected serialization paths, not every device, flag, codec or
combination. Broader complete-book cases are the next focused follow-up. EPUBCheck checks
format conformance, not processing geometry, KCC compatibility, visual quality, accessibility
completeness, performance or physical-device behavior. Keep those checks separate.

## Automatic checks and maintenance

[EPUB conformance](../../.github/workflows/epubcheck.yml) runs on relevant PRs and `main`
changes, and can be dispatched manually. The release workflow calls the same gate on the
tagged commit before packaging. It runs on Linux; the existing three-platform Rust CI
remains unchanged. On failure, CI retains generated books and reports for seven days,
not the downloaded Java tool or private manga pages.

Review the upstream release before updating the version/digest together. Verify the
official asset digest, rerun the positive cases and negative control, and verify remote
checks for the exact final commit. Download failures are not clean validations. EPUBCheck's
default whole-book rules are used; `-v` is not a whole-book version selector.
See the official [CLI](https://www.w3.org/publishing/epubcheck/docs/cli/) and
[report](https://www.w3.org/publishing/epubcheck/docs/report/) references for the checker contract.
