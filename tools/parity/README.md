# Comparing image processing with KCC

These tools run mangapress and [KCC](https://github.com/ciromattia/kcc) **12.0.0** on the same
images and compare their results. Use them for changes that affect processed pixels. They run
separately from `cargo test`.

## Run the comparison

You need the [repository's Rust toolchain](../../CONTRIBUTING.md#setup-and-checks) and Python 3.
From the mangapress repository root, install the
development-only comparison dependencies and clone the reference into a separate folder.
Use a virtual environment if you do not want to change your Python installation:

```sh
python -m pip install -r tools/parity/requirements.txt
git clone --depth 1 --branch v12.0.0 https://github.com/ciromattia/kcc ../kcc-reference
python tools/parity/parity.py --kcc ../kcc-reference
```

The driver supplies substitutes for KCC imports that the comparison does not use, so the full
KCC dependency set is not required. A failing comparison lists differences and exits nonzero.

- `--profile CODE` selects a built-in device profile; the default is `K11`.
- `--smoke` runs three representative scenarios: default processing, CBZ padding, and color
  output. It excludes the full matrix and exact dither/webtoon checks.
- `--only TEXT` runs scenarios whose names contain the text. Repeat it to select a union.
  A filter matching nothing fails; `--only dither` generates its reference pages even in a
  fresh work folder. It cannot be combined with `--smoke`.
- `--pages DIR` adds your own pages to the default and color-output scenarios.
- `--work DIR` changes the output folder; the default is the ignored `target/parity/`.

Inspect the generated PNGs from both tools when investigating a difference. Keep private pages
outside the repository; the built-in corpus uses generated images.

For example, check the DX's CBZ behavior or run the small matrix on a Kobo:

```sh
python tools/parity/parity.py --kcc ../kcc-reference --profile KDX --only CBZ --work target/parity/KDX
python tools/parity/parity.py --kcc ../kcc-reference --profile KoLC --smoke --work target/parity/KoLC
```

Use a separate `--work` folder for simultaneous runs. To check the driver's own bookkeeping:

```sh
python -m unittest discover -s tools/parity -p "test_*.py" -v
```

## What is checked

| Property | Expected match |
| --- | --- |
| Detected background and black-background flag | Exact |
| Output page count, order, and page/spread type | Exact |
| Page dimensions and grayscale/color classification | Exact |
| Decoded pixels | Mean difference at most 1.0 gray level, or 1.5 for color |

Palette dithering, webtoon joining, and webtoon splitting have separate pixel-exact comparisons.
The dither checks use 16-, 15-, and 4-level palettes.

The pixel tolerance accounts for JPEG encoding and decoding differences. A passing comparison
applies to the tested scenarios; it is not a guarantee for every image, option, or device.

## Limits

The main scenarios use the selected profile. Four explicitly labelled custom-resolution
regressions always use `KDX` or `KS3`. The upright-spread scenario always uses `KoC` (1072 × 1448),
matching the original Kindle 11 baseline without KCC's deliberate Kindle EPUB cap. These
fixed-profile cases are labelled with their actual code in the output.

Webtoon checks use the selected profile's effective EPUB target, obtained from KCC's own
option resolver, including the Scribe width cap. The exact dither check still
exercises all three palettes (`K11`, `K2`, `K1`), independently of the selected device.

The comparison does not verify EPUB markup and navigation, covers, labelled-spread joining,
or the encoded archive bytes. Relevant Rust tests cover those separately. See
[ADR 0013](../../docs/adr/0013-follow-a-named-kcc-release.md) for deliberate differences, including
PNG rather than GIF for quantized Kindle EPUB pages and the size of upright spreads.

## Automatic checks

[KCC parity](../../.github/workflows/parity.yml) runs on pull requests and pushes to `main`
that change the crates, Cargo manifests/lockfile, comparison tools, or that workflow. It uses
KCC 12.0.0 at commit `f127adbca992456e173d88ada18643eae66802fb` and the dependency versions in
`requirements.txt`: the full Kindle 11 matrix, including the DX and Scribe custom-resolution regressions,
then smoke checks for `K1`, `K2`, `KDX`, `KS3`, `KCS`, `KoLC`, and `RmkPPMove`. This spans all
three grayscale palettes, old Kindle defaults, large screens, color devices, and each family.
A separate `KS3` webtoon check also exercises its format-specific target.

It runs on Linux and supplements, rather than replaces, the three-platform Rust CI. There is
no schedule or latest-release monitor. `workflow_dispatch` also permits a manual run; reviewing
a new KCC release remains the contributor's responsibility below. Generated images and KCC's
checkout stay under ignored `target/` and are not shipped in the application.
The release workflow also calls this comparison on each tagged commit before packaging.
Changing `rust-toolchain.toml` triggers the PR/main comparison as well.

## Files

- [parity.py](parity.py): scenarios and comparisons.
- [kcc_oracle.py](kcc_oracle.py): calls KCC from its checkout.
- [make_corpus.py](make_corpus.py): generates the test images.
- [parity_dump.rs](../../crates/mangapress-core/examples/parity_dump.rs): runs mangapress's pipeline
  and writes pages for comparison.

The [licensing boundary](../../docs/adr/0007-gplv3-boundary-kcc-image-rs.md) applies here too.

## Updating the reference

Run the comparison against the proposed KCC tag. Review each difference before changing the
reference: follow the new behavior or record a deliberate difference in ADR 0013. Update the
implementation, regression tests, and `REFERENCE_KCC` in `parity.py` together.
