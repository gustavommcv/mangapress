# 1. Use Rust

## Status

Accepted (decided before this repository existed; recorded here for the
historical record).

## Context

KCC's CLI mode runs poorly when installed via pip on Arch Linux, because
`setup.py`/`requirements.txt` declare `PySide6` (Qt) as a hard dependency
even for CLI-only use, with no `extras_require` split. Three replacement
approaches were considered: Go, "clean" Python (KCC's own algorithms
without PySide6), and Rust.

- Go would need `cgo` + `libvips` to reach a comparable level of image-
  processing maturity, reintroducing a native dependency.
- A Python rewrite without PySide6 still needs an interpreter installed —
  it doesn't solve packaging/distribution on its own, only removes one
  dependency.
- Rust's `image` crate ecosystem is mature enough for this domain, and
  produces a genuinely static binary with no runtime to install.

## Decision

Rust, using the `image` crate ecosystem for the image-processing core.

## Consequences

Steeper contribution barrier for newcomers than Go/Python would have been.
Accepted deliberately: KCC itself doesn't have a large external
contributor base (as of this decision, ~40 open issues, not a
PR-swarm project), so optimizing for a technically sound solution took
priority over lowering the contribution barrier for a hypothetical influx
of contributors.
