# 4. Dual-license under MIT OR Apache-2.0

## Status

Accepted.

## Context

Upstream KCC is ISC-licensed at the repository level (though not
uniformly — see `0007-gplv3-boundary-kcc-image-rs.md`). mangapress doesn't
copy KCC's licensed code, so it isn't bound to ISC or any KCC-derived
license; the project is free to pick its own.

## Decision

Dual MIT/Apache-2.0, the de facto standard for Rust CLI tools (ripgrep,
fd, bat — the exact category of tool this project is explicitly modeled
on). Apache-2.0 additionally grants an explicit patent license that MIT
alone doesn't.

## Consequences

If, at some point, an ISC-licensed snippet from KCC actually gets ported
verbatim (rather than reimplemented from the algorithm description), that
specific file must retain the original ISC notice per its terms — dual
MIT/Apache-2.0 for the rest of the project doesn't remove that obligation
for a directly-copied file. See `0007-gplv3-boundary-kcc-image-rs.md`.
