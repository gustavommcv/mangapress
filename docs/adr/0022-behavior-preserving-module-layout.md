# 22. Keep module refactors separate from behavior changes

Date: 2026-10-10

## Status

Accepted through maintainer approval of the pull request containing this record.

## Context

The October refactor began with `main.rs` at 1,886 lines, including a conversion
function of about 1,150 lines. The EPUB writer and page pipeline also mixed
several responsibilities with large inline test modules. Changing one part
required reading unrelated code, even though the crate boundaries were sound.

The goal was readability and maintainability, not new conversion behavior.
The existing two-crate boundary (ADR 0003), processing rules, book builders,
safe output policy (ADR 0016), and machine protocol (ADR 0011) remain in place.

## Decision

### Core layout

- `ebook/epub/mod.rs` keeps `build_epub`, `EpubOptions`, and archive assembly.
  Private `package`, `navigation`, `page`, `spine`, and `identifier` modules
  handle OPF/container XML, NCX/nav/bookmarks, XHTML, page-side assignments,
  and the existing SHA-1/UUID-v5 implementation. No identifier dependency is added.
- `pipeline/mod.rs` keeps its public options and per-page orchestration.
  Private `cropping`, `sizing`, and `finishing` modules contain the existing
  stages; `pipeline::spread` keeps its public path and spread responsibilities.
  Existing crop, resize, contrast, color, quantization, and codec implementations
  are reused rather than replaced.
- The extracted tests for EPUB, pipeline, crop, input, JPEG, JPEG recoding, and
  natural sorting live in adjacent `tests.rs` files. Their module paths stay the
  same. The CLI's original tests remain in `main_tests.rs`.

### CLI layout

`main()` retains argument parsing, handshake, and final error handling.
`run()` handles profile listing and otherwise coordinates these private phases:

| Module | Responsibility and handoff |
| --- | --- |
| `configuration` | Resolve arguments, profile, format, dimensions, and pipeline options into `ResolvedConversion` |
| `inspection` | Read source entries and resolve metadata into `ReadInput` / `InputMetadata` |
| `preparation` | Filter pages, select covers, and join labelled spreads into `PreparedPages` |
| `planning` | Plan/stage the destination or finish a read-only dry run, returning `Outcome` / `PlannedOutput` |
| `processing` | Process chapters and webtoon pages with the existing core, returning `ProcessedBook` |
| `assembly` | Reuse cover and EPUB/CBZ/PDF builders to produce `AssembledBook` |
| `publication` | Consume completed bytes and the staged owner, preserve the distinct `BookCounts`, and emit the existing write/result reports |

`args` owns the CLI declaration; `discovery` and `reporting` share existing
helpers. `protocol` still owns event framing and failure types. `output` still
owns filesystem naming, staging, synchronization, and no-clobber publication;
the new phases coordinate it without implementing another filesystem policy.

These handoffs are private to the CLI, not new library or protocol APIs.
Owned source/book buffers move between phases or are borrowed where already
needed; the refactor adds no copy of the page or assembled-byte collections.

### Refactor boundary

1. Phase 1 contains only moves and module splits, retaining function bodies,
   public paths, and existing tests, apart from the necessary module wiring.
2. Phase 2 contains small CLI design improvements, one concern per PR, with
   a written reason and tests for each handoff and relevant failure path.
3. Neither phase changes flags/help, human output, exits, names, protocol fields
   or event order, pixels, or book payloads. Existing deliberate differences
   from the named reference are not fixed or widened during this work.
4. Verify formatting, locked all-target linting/tests, golden books and event
   streams, unchanged comparison counts, exact-head remote checks, and timing.
   Keep initial failures and timing uncertainty visible. A discovered defect
   or dependency change needs a separate PR and decision, not a refactor label.

## Consequences

The CLI entry point is now an outline of the conversion rather than its full
implementation; format and image details remain in the core. Contributors can
find a phase and its tests without a new crate, phase framework, or dependency.
The public paths and signatures of `build_epub`, `EpubOptions`, `process_page`,
and the other existing core interfaces are unchanged.

Size targets guide review, not automatic splitting. In particular, `output.rs`
retains its related policy and inline tests, and the pipeline keeps its existing
per-page orchestration together. Do not create tiny modules merely to meet a
line count. Readability improvements do not establish faster processing or
exhaustive compatibility; comparison and reader-test limits remain unchanged.
