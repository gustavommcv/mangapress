# Release checks

Releases still require maintainer approval. Bump the workspace version in a focused PR,
verify the exact commit's remote checks, and merge it into `main` before pushing its matching
`v<version>` tag. Do not move a published tag or rebuild a different commit under its name.

The tag workflow validates the Cargo version and checks that the tagged commit belongs to
`main`. It then calls the same CI and KCC comparison workflows used for PRs, on that exact
commit, with the extended pre-release parity cases enabled, before building the four existing
release targets. A failing validation, test,
advisory check, notice generation, comparison, or build blocks publication.

Only the publication job has repository write permission. It downloads the `mangapress-*`
package artifacts, not the separate CI license reports, generates checksums, and requires
the release tag to exist. Archive names and installer entry points are unchanged.

## Maintenance

- Rust 1.98.1 is the tested minimum and release compiler. The repository toolchain file is
  the source used by rustup in local development, CI, and release builds. Update it and the
  inherited Cargo `rust-version` together; rerun the three-platform checks and KCC comparison.
- External actions are pinned to commit SHAs with release labels. Review upstream changes
  before updating the pins. cargo-about 0.9.2, cargo-audit 0.22.2, and actionlint 1.7.12 are
  build/check tooling, not runtime dependencies.
- The RustSec database is intentionally current at each audit. Tool pins and a lockfile do
  not freeze security findings or GitHub-hosted runner images. Do not treat a database
  fetch failure as a clean audit.

Branch protection, Dependabot updates, and GitHub private vulnerability reporting are
repository settings or separate automation choices, not enabled by these workflow edits.
The [security policy](../SECURITY.md) provides the current private contact. Signing, SBOMs,
build provenance, new platforms, and scheduled parity monitoring remain outside this change.
