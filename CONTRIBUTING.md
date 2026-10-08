# Contributing to mangapress

Bug reports, documentation fixes, device testing, and code contributions are welcome. For bugs,
include the version, command, device profile, expected result, and actual output. A small
synthetic input or a description of the page layout is preferable to uploading manga pages.

## Setup and checks

Use Rust 1.98.1 with rustfmt and Clippy, as selected by
[rust-toolchain.toml](rust-toolchain.toml) and used in [CI](.github/workflows/ci.yml).
With rustup installed, run `rustup toolchain install --no-self-update` from the checkout.
This is also the declared minimum; compatibility with older compilers has not been tested.
From the repository root:

```sh
cargo build --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Use `cargo fmt --all` to apply formatting. To run the CLI from the checkout:

```sh
cargo run -p mangapress-cli -- --help
```

CI checks formatting, linting, and tests on Windows, macOS, and Linux. Release targets and
packaging are defined in [the release workflow](.github/workflows/release.yml).
Dependency changes also require the four target-specific
[license-notice checks](tools/licenses/README.md). CI and release packaging use the same
generator; review its reports rather than accepting missing notices or generic fallback text.
Installer changes also require the [offline installer checks](tools/installers/README.md),
which run the actual scripts with native hashing and extraction on their supported platforms.

## Where to make a change

- `crates/mangapress-core/`: image processing, profiles, metadata, and book formats.
- `crates/mangapress-cli/`: options, input/output handling, and terminal or JSON output.
- `tools/parity/`: comparisons against the named KCC reference release.

Keep the core usable independently of the CLI. See the [architecture decisions](docs/adr/README.md)
for the existing boundaries and reasons behind them.

## Tests and fixtures

Add regression tests for changed behavior and relevant error cases. Keep fixtures small and
synthetic; see [tests/fixtures/README.md](tests/fixtures/README.md). Real pages used to investigate
a problem belong in the ignored `tests/fixtures/real/` folder and must not be committed.

For image-processing or book-output changes, also run the [KCC comparisons](tools/parity/README.md).
Install the small Python dependency set and provide a separate KCC checkout, as described there.
They supplement Rust tests and cover only their [listed scenarios and gaps](tools/parity/coverage.md).
The comparison workflow runs the full Kindle 11 matrix, representative device checks, and
small CLI-to-book cases for relevant PRs; release checks add the extended cases.
Verify that run for the exact commit as well as the three-platform Rust CI.

KCC 12.0.0 is the current reference. Preserve the implementation and licensing boundary in
[ADR 0007](docs/adr/0007-gplv3-boundary-kcc-image-rs.md), and follow
[ADR 0013](docs/adr/0013-follow-a-named-kcc-release.md) when considering differences or a new
reference release.

Changes to JSON events must preserve the [machine protocol](docs/machine-protocol-v1.md),
or explicitly version a breaking change.

## Pull requests

Keep each PR focused and explain the change and its verification. Write code comments,
documentation, commit messages, and PR descriptions in English. Check instructions against the
current CLI and workflows. Preserve accepted ADRs and audit evidence; record a changed decision
in a new ADR when it needs one.

CI also runs pinned cargo-audit and actionlint tools. Security advisories are fetched on each
run, not frozen with the compiler. Review findings rather than adding blanket ignores.
For workflow changes, check the [release gates](docs/releases.md) too. Update action SHAs
only after reviewing the upstream release and verifying all affected remote workflows.

Before merging, verify the remote checks for the exact PR commit and wait for the maintainer's
approval. Passing local checks does not replace a successful remote run. If the run cannot be
verified, report the commit and the visibility problem rather than treating the work as complete.

The [October 2026 audit](docs/audit-2026-10.md) records findings at its stated snapshot; check linked
follow-ups and current code before picking up a finding.
