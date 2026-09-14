# 11. Add a versioned JSON Lines event stream

## Status

Accepted.

## Context

mangapress has a real external orchestrator now: Mangabound bundles a pinned release binary and
must show progress and failures at the stage, chapter, and page that produced them. Terminal prose
is intentionally written for a person and is not a safe integration contract. A single JSON result
written after conversion would also be insufficient because page processing is the long-running
part of the program and must be cancellable and observable while it runs.

ADR 0009 declined structured output when no consumer existed. That premise has changed, but unlike
Mangabind's ADR 0009, structured output was only one independently deferred item in mangapress's
broader CLI-conventions decision. This ADR supersedes that one conclusion, not the rest of ADR 0009.

## Decision

- Add `--json-events`. It writes newline-delimited JSON objects to stdout and suppresses ordinary
  progress prose. Without the flag, all existing human output, warnings, and exit-code behavior is
  retained.
- Add `--protocol-version`, requiring no input, for a machine-readable compatibility handshake.
  Protocol versioning is independent of the release version.
- Every event contains `protocol_version`, `tool`, `tool_version`, a contiguous one-based `sequence`,
  and `type`. JSON Lines is used so a consumer can process events immediately without waiting for
  the final book.
- Version 1 includes stage transitions, chapter transitions, serialized per-source-page completion,
  warnings, structured errors, device profiles, and a final result with the output path and byte
  count. Parallel page work remains parallel; only the tiny progress callback is serialized so
  event sequence and completed counts cannot race or move backwards.
- Errors and warnings use stable `code`, `stage`, context, `recoverable`, and user-facing `message`
  fields. Technical `diagnostic` text is supplementary. Invalid CLI usage still exits 2 and runtime
  failures still exit 1; machine mode emits an error event before termination whenever possible.
- `--list-profiles --json-events` emits one structured profile event in declaration order for every
  supported device, followed by a result event. Human `--list-profiles` remains its existing table.
- Consumers must ignore unknown fields, event types, and issue codes. Adding them is compatible.
  Removing or retyping a field, changing framing or documented meaning, or making sequence ordering
  non-monotonic requires a new protocol version and ADR.

The normative contract is [`docs/machine-protocol-v1.md`](../machine-protocol-v1.md).

## Consequences

The CLI remains pleasant to run directly while Mangabound can render real progress and actionable
failures without scraping stderr. The machine stream is now release surface: its framing, fields,
ordering, and failure behavior require regression tests, including the repository's optional real
Mangabind-produced fixture validation.

Event emission adds a short serialized write after each source page. Image processing remains
parallel and dominates that cost by orders of magnitude.
