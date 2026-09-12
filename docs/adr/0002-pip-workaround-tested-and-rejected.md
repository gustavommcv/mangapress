# 2. Tested the pip/venv workaround; proceeding with the rewrite anyway

## Status

Accepted.

## Context

Before writing any new code, we tested whether KCC's Arch Linux packaging
problem could be solved without a rewrite at all, per the project's own
"don't reinvent what already works" rule.

Findings (reproduced live on a real Arch Linux environment, not just read
from source):

1. `KindleComicConverter` is **not published on PyPI** — `pip install
   kindlecomicconverter` fails outright (`No matching distribution
   found`). The only PyPI listing under that name family is
   `KindleComicConverter-headless`, an unofficial fork abandoned since
   March 2020 with a stale, much smaller dependency set than current
   upstream. Upstream's own README documents installing from source
   (`git clone` + `venv` + `pip install -r requirements.txt`), not via
   PyPI.
2. The *real* workaround — clone from source, create a venv, install
   `requirements.txt` minus `PySide6` (`Pillow`, `psutil`, `requests`,
   `python-slugify`, `packaging`, `mozjpeg-lossless-optimization`,
   `natsort`, `numpy`, `PyMuPDF`) — **works completely**. `kcc-c2e.py
   --help` runs, and a full synthetic manga conversion (crop, resize,
   manga-mode, EPUB with correct per-chapter TOC) succeeds end-to-end,
   with zero PyQt/PySide6 installed.
3. One additional real system dependency surfaced: `comicarchive.py`
   hard-requires the external `7z` binary for **any** archive extraction,
   even a plain `.cbz`/ZIP that Python's stdlib `zipfile` could read
   without it — there is no pure-Python extraction fallback. Fixed by
   `sudo pacman -S p7zip`.

So the literal "roda mal no Arch" problem has a known, working, low-effort
fix: run from source without PySide6, with `p7zip` installed.

## Decision

Proceed with the Rust rewrite anyway. The packaging bug that originally
motivated this project turned out to be fixable in five minutes, but the
project's other stated goals don't depend on that bug existing:

- A genuinely static, dependency-free binary (no Python interpreter, no
  venv to manage) — the source workaround still requires Python installed
  and a venv per machine.
- Automated regression tests for the image-processing heuristics — KCC has
  none today (confirmed: no `tests/`, no test-running CI job, only CodeQL
  static analysis and release packaging).
- The technical/architectural interest of doing this well in Rust, per
  the project's original non-functional requirements.

## Consequences

This is a "want to," not a "have to" — worth remembering if scope pressure
ever makes finishing feel urgent. The fallback (run KCC from source without
PySide6) is a legitimate, documented escape hatch if the rewrite stalls.
