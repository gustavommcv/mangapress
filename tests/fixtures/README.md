# Fixtures

Use small synthetic images, such as checkerboards, gradients, or noise with known properties.
Keep fixtures reproducible and suited to the behavior being tested.

Real manga or comic pages used for local investigation belong in the ignored `real/` folder.
Do not commit them or make automated tests depend on them; reproduce the relevant properties
with a synthetic fixture instead.

`folder_links.rs` shares native link helpers between library and CLI tests. Symbolic-link
fixtures can be unavailable on Windows without the required privilege; only that specific
OS error permits a skip. Linux/macOS link tests and Windows junction tests must execute.
The junction helper uses PowerShell only to create temporary test links, not in production.
