# 17. Follow folder links only to regular files inside the selected input

Date: 2026-10-07

## Status

Accepted by the maintainer during the October audit.

## Context

The folder reader used `Path::is_dir`, which follows links, before recursively reading
entries. A directory link could create a cycle; a file link could silently include an
image outside the selected folder in a book that is then shared.

KCC 12.0.0 prepares folder input with `shutil.copytree` and its default of following
links. That is not a desirable behavior to reproduce here. Mangabind already permits
only links to regular files inside its selected input (its ADR 0014).

## Decision

- Apply the same containment policy recursively to folder input, including linked
  `ComicInfo.xml`. Resolve the input root once and each link's complete target with
  Rust's `canonicalize`; compare path components, not a textual prefix.
- Follow a descendant link only if its resolved target is a regular file inside the
  real input root. Read the checked target, retaining the link's own relative path
  for natural ordering and chapter attribution. Existing byte limits still apply.
- Skip links outside the input, links whose targets cannot be resolved, and links to
  anything other than a regular file. Do not recurse into directory links. This also
  covers junctions recognized as links by Rust's native Windows filesystem APIs.
- The explicitly selected root may itself be a directory link; its real path defines
  the boundary. An explicitly selected CBZ link remains a supported file input.
- Return typed skipped-link diagnostics from the core reader, without terminal or
  JSON output in the library. The CLI emits one `link_skipped` warning per rejection
  at `inspect`, in natural path order, on both dry-run and conversion. Quiet mode
  still retains warnings. Reports contain the link's own path and a categorical
  reason, never the target's path or contents.
- Count rejected links separately from unrecognized non-image files. If no pages
  remain, warnings precede the existing `no_page_images` error; a truly empty input
  retains `input_empty`. No new flags, dependencies, or protocol version are needed.

## Consequences and limits

Users who intentionally link to external images must copy them into the input instead.
Ordinary input keeps its bytes, natural ordering, and image-processing behavior.
The generic `read_folder` convenience function applies the policy and retains its
`Vec<SourceEntry>` return type; `read_book` exposes the skipped-link diagnostics.

This is input hygiene, not a filesystem sandbox. Hard links and mounts not classified
as symbolic links are not detected. Concurrent changes to the root, ancestors, or
checked target can defeat a check made before opening. Separately supplied covers,
spread-label files, and existing sibling-cover discovery are outside this folder walk.

Tests create small native links and check chains, relative/absolute targets, directory
cycles, broken links, linked roots, metadata, and CLI warnings/page counts. Linux and
macOS require those tests to run; Windows can skip symbolic-link fixtures only when
the OS reports missing symlink privilege. Windows junction tests require no Developer
Mode and exercise the library and real CLI. No system settings are changed.

## References

- [Mangabind's input-link policy](https://github.com/gustavommcv/mangabind/blob/main/docs/adr/0014-links-stay-inside-the-input.md).
- [KCC 12.0.0 folder workspace](https://github.com/ciromattia/kcc/blob/v12.0.0/kindlecomicconverter/comic2ebook.py#L938).
- [Python copytree defaults](https://docs.python.org/3/library/shutil.html#shutil.copytree).
- [Rust directory-entry types](https://doc.rust-lang.org/std/fs/struct.DirEntry.html#method.file_type) and [canonical paths](https://doc.rust-lang.org/std/fs/fn.canonicalize.html).
