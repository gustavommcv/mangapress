# Release dependency notices

Release archives include `DEPENDENCY-LICENSES.txt`, generated for their target from the
locked CLI dependency graph by [cargo-about](https://github.com/EmbarkStudios/cargo-about).
This is build tooling, not a runtime dependency. It includes build dependencies as a
conservative superset, but excludes development-only dependencies. The project's own
licenses and existing KCC/Pillow notices ship separately and are unchanged.

## Generate locally

Install the pinned tool and run from the repository root with Python 3 available:

```sh
cargo install cargo-about --version 0.9.2 --locked
python tools/licenses/generate.py --target x86_64-pc-windows-msvc
python -m unittest discover -s tools/licenses -p "test_*.py"
```

Other release targets are `x86_64-unknown-linux-musl`, `x86_64-apple-darwin`, and
`aarch64-apple-darwin`. Cross-target notice generation needs metadata, not a linker.
The output is `target/dependency-notices/<target>/DEPENDENCY-LICENSES.txt`.
`--cargo-about PATH` can select a separately installed cargo-about executable.

CI generates all four reports using the same action as release packaging, and uploads
them for review. These checks do not build extra release binaries or publish releases.
Generated texts are not committed; each release regenerates them from its own lockfile.

## Updating dependencies

`about.toml` chooses licenses from the alternatives declared by dependencies; it does not
change mangapress's MIT/Apache-2.0 license. cargo-about gathers the files and resolves SPDX
expressions. The template preserves their text and associates each notice with crate
names and versions. No local source paths are distributed.

Review all four reports after dependency changes. Generation uses `--locked --fail`,
then checks that every external notice has a source file and that its text and attribution
survive rendering. This extra check matters: cargo-about can fall back to a generic SPDX
text without the original copyright, even with `--fail`. An unrecognized license, missing
source notice, or rendering omission stops the job instead of producing a partial bundle.

Clarifications use SHA-256 hashes of original files, not rewritten license text:

- `bzip2-sys` also distributes bzip2's native-library license, omitted from its crate metadata.
- `brotli-decompressor` ships a BSD-3-Clause notice; use it instead of the manifest's legacy
  slash-separated alternatives. `alloc-stdlib` omits this notice from its crate archive;
  retrieve it from the upstream commit recorded by the package.
- `constant_time_eq`, `ouroboros`, and Windows bindings need explicit license filenames.
- Pathfinder packages omit their license files. Use upstream files at the package commits;
  geometry's recorded commit is unavailable, so its clarification explicitly pins the
  reachable simd commit, which still declares geometry 0.5.1.
- `jpeg-encoder` is `(MIT OR Apache-2.0) AND IJG`: the IJG's license has no file of its own in
  the crate, only the header of `src/fdct.rs`, so that header (from the line that begins it to the
  last line of the license text) is the notice, checked by the hash of that text. The IJG asks
  for an acknowledgement in the documentation as well; `THIRD-PARTY-NOTICES.md` has it.
- The tool's existing `rustix` workaround handles that crate's `COPYRIGHT` layout.

If these files or package commits change, review upstream licensing and update the hashes
as needed. Do not remove the source check or accept generic fallback text to get CI green.
