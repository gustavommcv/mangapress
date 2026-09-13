# 9. Adopt relevant CLI conventions from clig.dev

## Status

Accepted.

## Context

mangapress's CLI grew flag-by-flag as each pipeline feature landed (see `mangapress-cli/src/args.rs`'s
own module doc: "flags are added as their backing feature gets implemented ... not preemptively"),
without ever being checked against established CLI design conventions. Audited against
[clig.dev](https://clig.dev)'s guidelines, the same way Mangabind was (see that project's own
[ADR 0009](https://github.com/gustavommcv/mangabind/blob/main/docs/adr/0009-cli-conventions-and-batch-mode.md)
for the format this one follows — not its content, since the two tools' actual usage profiles
differ in a way that changes several conclusions).

**What already holds up, confirmed by reading the code and by actually running the built binary
against a real 65 MB / 182-page volume** (`Chainsaw Man - Vol.01.cbz`), not just inferred from it:

- `--version` prints a real, accurate version (`mangapress 0.1.0`, sourced from `Cargo.toml` via
  clap's `version` attribute, not a hardcoded string) — and now that releases are tagged
  (ADR-adjacent to the README/release-pipeline work), it'll track real releases too.
- `-h`/`--help`/`--version` all short-circuit correctly regardless of other flags passed (verified:
  `mangapress --profile BOGUS --help` still prints help and exits 0), and exit 0 — this is clap's
  derive behavior, inherited for free.
- Exit codes are already meaningfully split: clap's own usage errors (missing `<INPUT>`, bad enum
  value) exit **2**; a valid invocation that fails at runtime (unknown profile, missing input file)
  exits **1** via `anyhow`. No work needed here.
- Errors already go to `stderr`, not `stdout` (verified by redirecting each away independently), and
  are already rewritten into one-line, human-readable messages (`Error: unknown device profile
  'BOGUS'`, `Error: input path does not exist: ...`) rather than raw panics or debug dumps, for every
  path exercised.
- `-o`/`--output` already matches the standard flag name (`sort`, `gcc`).
- Ctrl-C (`SIGINT`) already terminates near-instantly with no partial output file — measured directly
  (see below), not assumed.

**Concrete gaps found:**

- All routine progress messages (the startup banner, chapter/page counts, per-chapter progress
  dots, the final "wrote ..." line) go to `stdout`, not `stderr`. None of it is this program's actual
  *output* — that's the file written to disk — so per clig.dev it belongs on `stderr`, freeing
  `stdout` for the day mangapress might need to emit something machine-consumable, and so
  `mangapress ... 2>/dev/null` behaves as most users would expect ("just the file, no chatter") instead
  of print output still leaking through because it's on the wrong stream entirely.
- **Progress granularity is per-chapter, not per-page**, and this is a real, measured problem, not a
  cosmetic one — unlike Mangabind, which only copies/reorganizes files, mangapress does real
  per-page image work (crop, resize, contrast, optionally FFT-based rainbow-artifact removal and
  dithering), so "one dot per chapter" can mean a genuinely long silence. Measured directly against
  the real fixture:
  - Default flags: chapters complete every 0.6–0.8s (fine).
  - `--forcepng --eraserainbow` (a realistic heavier configuration): **1.6–2.3s of total silence
    between each chapter's dot**, and that gap scales with chapter size — a manga with a few large
    chapters instead of many small ones, or a larger volume, would push this well past the point
    where clig.dev's "show progress if something takes a long time" stops being optional.
- `--profile`'s help text tells users to go read `mangapress-core::profile::PROFILES` — a Rust
  module path — to find the full list of ~40 device codes. That's meaningless to someone who only
  has the compiled binary, which is mangapress's whole distribution model. There's also no
  suggestion when a profile code is mistyped (`Error: unknown device profile 'Kv5'` gives no hint
  that `KV` or `KPW5` exists).
- No `-n`/`--dry-run`. Unlike Mangabind (cheap file copies), mangapress's actual page-processing
  loop is the expensive part of the program — validating input, profile, and resolved metadata
  without running it is a real, cheap thing to offer before someone commits to a long run.
- No `-q`/`--quiet`.

**Guidelines considered and deliberately not adopted**, because they don't fit this tool's actual
usage pattern:

- **A full progress-bar/spinner library (e.g. with ETA).** Real measured runs are single-digit to
  low-tens of seconds, not minutes — a plain, periodically-updated "page N/total" counter gets
  nearly all the user-facing benefit of a progress bar without a new dependency, without needing to
  handle terminal-width truncation, and without the "don't animate when not a TTY" complexity a real
  progress-bar crate has to solve properly. Revisit if typical volumes get materially bigger/slower.
- **Custom `SIGINT` handling / a "cleaning up..." message.** Tested directly: sending real `SIGINT`
  (not a backgrounded shell job's pre-ignored one — confirmed that distinction by inspecting
  `/proc/<pid>/status`'s signal masks) to a mid-run process kills it in ~7ms via the default OS
  disposition, and no output file exists yet at that point regardless of when the signal lands,
  because `main.rs` only calls `std::fs::write` once, at the very end, after the entire book is
  already built in memory. There is nothing to clean up. Installing a custom handler here would add
  a dependency and real risk (a handler that doesn't cover every exit path, or that itself hangs)
  for zero measurable gain over what already happens for free. A real terminal already echoes `^C`
  on interrupt at the TTY layer regardless of what the program does, so the user already gets
  visual confirmation the keystroke landed.
- **`--json`/`--plain` machine-readable output.** mangapress's actual output is a file on disk, not
  data printed to a stream — there's no existing "record" to structure, and no evidence anyone
  scripts around its stdout today. Adding a second output format to keep in sync for a hypothetical
  consumer is exactly the premature scope ADR 0006/0008 already declined for other reasons.
- **Config files / environment variables.** Every mangapress setting is the
  "varies-per-invocation" kind clig.dev itself says belongs on flags (cropping aggressiveness,
  format, profile, ...), not the "stable across invocations" kind that justifies a config layer.
- **Man pages, shell completions, analytics.** Maintenance surface with no demonstrated need at
  this project's size; the README (already rewritten to cover install/usage) is the right amount of
  documentation for now.
- **Batch mode (processing multiple volumes in one invocation).** Mangabind added this because its
  real usage pattern — a whole library of manga folders — made per-manga manual invocation a
  genuine chore. mangapress's role in the pipeline is converting one already-assembled volume at a
  time; no comparable "library of volumes in one go" use case has come up. If it ever does, the
  design constraint carries over unchanged from Mangabind's ADR 0009: an explicit `--batch` flag,
  never auto-detected (a folder of chapter subfolders and a folder of volume `.cbz`s aren't always
  distinguishable from the outside), and one bad volume must not abort the rest.
- **`--list-profiles` as a `--json`-only or config-driven feature.** Keeping it a plain flag that
  prints a human-readable table is enough; there's no current consumer that would need it
  structured.

## Decision

- Route every routine progress message in `main.rs` to `stderr` (`eprintln!`/`eprint!` instead of
  `println!`/`print!`). Errors already went to `stderr`; this makes the whole stream split correct.
- Replace the per-chapter progress dot with a per-page counter (`\rprocessing page N/total`,
  carriage-return-updated) when `stderr` is a terminal, falling back to a plain line printed once
  per chapter (no `\r`) when it isn't — so redirecting to a log file doesn't fill it with carriage
  returns.
- Add `--list-profiles`, printing every device code, display name, and resolution as a plain table;
  update `--profile`'s help text to point at it instead of a Rust module path. When an unknown
  profile is passed, suggest the closest known code by edit distance in the error message.
- Add `-n`/`--dry-run`: run everything up through resolving metadata and grouping chapters (all
  cheap), print what would be produced (target resolution, format, output path, chapter/page
  counts, resolved title/author), and stop before the page-processing loop and before writing
  anything.
- Add `-q`/`--quiet`: suppresses the routine progress lines (now on `stderr`); warnings and errors
  remain visible either way.

## Consequences

The CLI now matches the conventions users of other well-designed CLI tools expect
(`stdout`/`stderr` split, real per-page progress, `--dry-run`, `--quiet`, a discoverable profile
list) in the specific places where mangapress's own usage profile — real, sometimes-slow image
processing, run directly by a human rather than scripted — actually calls for them, without taking
on machine-readable output formats, config files, batch mode, or a progress-bar dependency this
project has no evidence it needs yet.
