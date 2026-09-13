# 10. Defer color (`--forcecolor`) output

## Status

Accepted.

## Context

mangapress converts every page to grayscale as the very first step of `process_page`
(`.to_luma8()`, before crop/resize/contrast/anything else runs) — regardless of the source
image's own color, and regardless of the chosen `--profile`. This includes profiles for
genuinely color-capable e-ink panels this project already lists: `KCS` (Kindle Colorsoft),
`KoCC` (Kobo Clara Colour), `KoLC` (Kobo Libra Colour) — all E Ink Kaleido displays. Picking one
of these today gets the right target resolution and nothing else color-specific; the output is
flattened to grayscale exactly like every other profile.

For the overwhelming majority of manga — traditionally printed and scanned in black and white,
with at most an occasional color cover or insert page — this loses nothing there was to lose, and
matches this project's primary use case. It only actually matters for content that's meaningfully
color throughout: webtoons/manhwa (typically full-color) or manga with extended color runs, read
on one of the color-capable profiles above. mangapress also has no webtoon mode
(KCC's `--webtoon`, which changes spread/split handling for tall vertical-scroll strips) — the two
gaps compound for that specific use case, though they're independent decisions.

Read directly from real upstream (`image.py` at the pinned research commit
`ea532c709b72a994fd9219c3bb7cd3f1df08027b`) to understand what supporting this properly would
actually involve, not guessed at:

- `colorCheck()`: a per-page heuristic (`calculate_color()`) deciding whether a page has
  meaningful color content at all — `--forcecolor` alone doesn't force color output for a page
  that's genuinely grayscale; `colorOutput = self.color and self.opt.forcecolor`.
- Gamma correction is forced to a no-op for color pages (`if self.gamma != 1.0 and self.color:
  gamma = 1.0`), and autocontrast is skipped for color pages unless `--colorautocontrast` is also
  passed — both are simplifications relative to the grayscale path, not extra work.
- The rainbow-artifact eraser has a distinct color path: convert to YUV, run the same
  frequency-domain filtering on the Y (luminance) channel only, reconstruct RGB from the filtered
  Y plus the untouched U/V. mangapress's own `rainbow.rs` already documents (and, this session,
  empirically validated against real upstream via numpy) that the filtering math itself is
  identical either way — only the channel it's applied to differs.
- Quantization (`--forcepng`) drops color by default even when `colorOutput` is true, converting
  to RGB and quantizing against a grayscale-derived palette; genuine color-palette quantization
  only happens with the additional `--force-png-rgb` flag, which this research didn't dig into
  further since it's a refinement on top of basic color support, not a prerequisite for it. Modern
  Kaleido panels do their own color-gamut reduction in firmware (unlike the old Kindles' genuinely
  fixed 4/15/16-level grayscale, which needed pre-quantization to display correctly at all) — so a
  reasonable first version can very likely skip color-palette quantization entirely and just ship
  standard RGB JPEG for color pages, letting the device handle final reduction, without the
  fidelity loss that skipping quantization would cause on the old fixed-palette grayscale Kindles.

## Decision

Not implemented now. This is a "not yet," not a permanent decision like MOBI/AZW3 (ADR 0008) —
there's no external tool this project would instead point users at; it's just real, currently
unjustified engineering cost for this project's actual primary use case. Sketching the shape of an
eventual implementation here so it's a concrete starting point later, not a vague someday:

- Crop detection algorithms need no changes: they can keep working on a grayscale proxy of the
  page purely to compute the crop box, then apply that same box to the real (color) pixels for
  output. Only the final pixel-manipulation calls (`apply_crop`, resize's `fit`/`pad`/`contain`/
  `stretch`, and encoding) need to actually handle `RgbImage`, not just `GrayImage`.
- `contrast.rs` and `quantize.rs` are skipped entirely for color pages, matching upstream's own
  simplifications above — no new work there.
- `rainbow.rs` needs the YUV luminance-only path added; the exact RGB<->YUV matrices and filtering
  order are already transcribed from real upstream source in this ADR's research and in
  `rainbow.rs`'s own module doc.
- `PipelineOptions`/`finish_page` need a per-page color-or-grayscale branch (decided by whether the
  decoded source image is actually RGB, not by replicating upstream's statistical `colorCheck()`
  heuristic — simpler, and "the source already has color pixels" is a good enough signal for a
  first version).
- `mangapress-cli` needs `--forcecolor`.

Rough shape: touches `pipeline/mod.rs`, `resize.rs`, `rainbow.rs`, `crop/mod.rs` (call sites, not
algorithms), `args.rs`/`main.rs`, plus new color fixtures for each — an architecture change on the
order of several hundred lines across 6-8 files, not a small patch. The color-capable device
profiles (`KCS`/`KoCC`/`KoLC`) stay as they are; they're already correct for resolution/palette
metadata and simply don't get color-specific processing yet.

## Consequences

The README states plainly that mangapress converts every page to grayscale today, including on
color-capable profiles, so someone choosing a Kaleido device for color content doesn't discover
that the hard way. Revisit this ADR (not write a new one) if a real webtoon/color-manga use case
shows up — the research above is the starting point, not a decision that needs re-litigating from
scratch.
