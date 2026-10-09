# Architecture Decision Records

1. [Use Rust](0001-language-rust.md)
2. [Tested the pip/venv workaround; proceeding with the rewrite anyway](0002-pip-workaround-tested-and-rejected.md)
3. [Two-crate workspace: `mangapress-core` (lib) + `mangapress-cli` (bin)](0003-workspace-layout.md)
4. [Dual-license under MIT OR Apache-2.0](0004-license-dual-mit-apache.md)
5. [The Mangabind chapter-subfolder contract, and fixing a KCC bug while preserving it](0005-mangabind-contract.md)
6. ~~[Defer MOBI/AZW3 output](0006-mobi-azw3-deferred.md)~~ — superseded by 8
7. [Treat KCC's `image.py` and `dualmetafix.py` as specification, not source to port](0007-gplv3-boundary-kcc-image-rs.md)
8. [MOBI/AZW3 is permanently out of scope](0008-mobi-azw3-permanently-out-of-scope.md)
9. [Adopt relevant CLI conventions from clig.dev](0009-cli-conventions.md) — machine output updated by 11; output safety updated by 16
10. ~~[Defer color (`--forcecolor`) output](0010-color-output-deferred.md)~~ — superseded: color output is implemented
11. [Add a versioned JSON Lines event stream](0011-versioned-json-lines-events.md)
12. [A two-level table of contents for a combined series, EPUB only](0012-nested-toc-for-combined-volumes.md)
13. [Follow a named KCC release exactly, and leave out what exists only for Amazon's converter](0013-follow-a-named-kcc-release.md)
14. [Defer WebP output, and smaller JPEG](0014-webp-output-deferred.md)
15. [Bound input reads and check image dimensions before decoding](0015-bounded-input-reads.md)
16. [Follow KCC's non-overwriting output names and stage completed writes](0016-safe-output-planning-and-publication.md)
17. [Follow folder links only to regular files inside the selected input](0017-folder-links-stay-inside-the-input.md)
18. [Use the standard EPUB property for centered spine items](0018-standard-centered-spine-property.md) — narrow exception to 13
19. [Differences from KCC found by the October 2026 comparison: kept, and left open](0019-differences-from-kcc-found-by-the-october-comparison.md) — adds to 13
