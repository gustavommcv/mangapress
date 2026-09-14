# Architecture Decision Records

1. [Use Rust](0001-language-rust.md)
2. [Tested the pip/venv workaround; proceeding with the rewrite anyway](0002-pip-workaround-tested-and-rejected.md)
3. [Two-crate workspace: `mangapress-core` (lib) + `mangapress-cli` (bin)](0003-workspace-layout.md)
4. [Dual-license under MIT OR Apache-2.0](0004-license-dual-mit-apache.md)
5. [The Mangabind chapter-subfolder contract, and fixing a KCC bug while preserving it](0005-mangabind-contract.md)
6. ~~[Defer MOBI/AZW3 output](0006-mobi-azw3-deferred.md)~~ — superseded by 8
7. [Treat KCC's `image.py` and `dualmetafix.py` as specification, not source to port](0007-gplv3-boundary-kcc-image-rs.md)
8. [MOBI/AZW3 is permanently out of scope](0008-mobi-azw3-permanently-out-of-scope.md)
9. [Adopt relevant CLI conventions from clig.dev](0009-cli-conventions.md)
10. [Defer color (`--forcecolor`) output](0010-color-output-deferred.md)
11. [Add a versioned JSON Lines event stream](0011-versioned-json-lines-events.md)
