# Fixtures

Only synthetic or procedurally generated images belong here (checkerboards,
gradients, random noise with known statistical properties, etc.) — never
real manga/comic pages. This mirrors the rule already followed in
[Mangabind](https://github.com/gustavommcv/mangabind): real copyrighted
fixtures are useful for manual verification but must never be committed.

If you need a real fixture to debug something locally, keep it under
`tests/fixtures/real/` (gitignored) and describe how to reproduce a
synthetic equivalent before submitting a PR that depends on it.
