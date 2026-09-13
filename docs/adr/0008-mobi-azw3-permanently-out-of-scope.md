# 8. MOBI/AZW3 is permanently out of scope

## Status

Accepted. Supersedes [0006-mobi-azw3-deferred.md](0006-mobi-azw3-deferred.md).

## Context

ADR 0006 deferred MOBI/AZW3 as a "not implemented yet" gap, mainly for
lack-of-crate/effort reasons. Revisiting it: the project's actual reading
pipeline (see the top-level `README.md`) is HakuNeko → Mangabind →
mangapress → **KOReader**, not a physical Kindle running its own firmware.
KOReader already reads fixed-layout EPUB well, which is why EPUB was
already the project's stated priority output format — MOBI/AZW3 only ever
mattered as a way to get content onto Kindle hardware's *native* reading
app, which was never this project's actual target to begin with.

## Decision

MOBI/AZW3 is not a future roadmap item, not just an unimplemented v1 gap.
`mangapress-cli`'s `--format` flag will not grow a MOBI/AZW3 option.

## Consequences

If a real need for native-Kindle-firmware output ever materializes (e.g.
a reader who hasn't jailbroken their device for KOReader), the right
answer is almost certainly "run the existing pip/venv KCC workaround
documented in `0002-pip-workaround-tested-and-rejected.md` for that one
conversion," not adding MOBI/AZW3 to this project — which exists to solve
problems that workaround doesn't (a static binary, real tests), not to
reach feature parity with upstream on formats outside this project's own
actual use case.
