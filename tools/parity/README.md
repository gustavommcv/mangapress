# Comparing image processing with KCC

These tools run mangapress and [KCC](https://github.com/ciromattia/kcc) **12.0.0** on the same
images and compare their results. Use them for changes that affect processed pixels. They run
separately from `cargo test`.

## Run the comparison

You need a Rust toolchain and Python 3 with Pillow and NumPy. From the mangapress repository
root, clone the reference release into a separate folder:

```sh
git clone --depth 1 --branch v12.0.0 https://github.com/ciromattia/kcc ../kcc-reference
python tools/parity/parity.py --kcc ../kcc-reference
```

The driver supplies substitutes for KCC imports that the comparison does not use, so the full
KCC dependency set is not required. A failing comparison lists differences and exits nonzero.

- `--only TEXT` runs scenarios whose names contain the text.
- `--pages DIR` adds your own pages to the default and color-output scenarios.
- `--work DIR` changes the output folder; the default is the ignored `target/parity/`.

Inspect the generated PNGs from both tools when investigating a difference. Keep private pages
outside the repository; the built-in corpus uses generated images.

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

The main scenarios use Kindle 11 (`K11`, 1072 × 1448). The upright-spread scenario uses a Kobo
profile at the same resolution to account for a deliberate difference from KCC.

The comparison does not verify EPUB markup and navigation, covers, labelled-spread joining,
or the encoded archive bytes. Relevant Rust tests cover those separately. See
[ADR 0013](../../docs/adr/0013-follow-a-named-kcc-release.md) for deliberate differences, including
PNG rather than GIF for quantized Kindle EPUB pages and the size of upright spreads.

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
