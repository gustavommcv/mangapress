# 23. Give a page's wrapper no line height, so KOReader does not draw the page 12 px short

Date: 2026-10-09

## Status

Proposed on 2026-10-09. The maintainer approves this record by merging the pull request that
carries it. It is a narrow exception to [ADR 0013](0013-follow-a-named-kcc-release.md), which
follows KCC's page markup.

## Context

KCC, and mangapress after it, write each page as an inline `<img>` with its pixel size, inside
`<div style="text-align:center;">`, under two stylesheet rules (`@page` and `body`). ADR 0013 chose
that markup because the percentage-sized block it replaced was stretched by KOReader.

An inline image sits on the baseline of a line, and a line keeps room below the baseline for the
font's descent. KOReader's engine (crengine) counts that room against the height it allows the
image: it limits the image to the page area minus what the surrounding line adds
(`getSurroundingAddedHeight`), and the width follows the height. The area KOReader has for a page is
already smaller than the screen (its margins and, unless "Overlap status bar" is on, its status
bar). The line then takes 12 px more, at every setting.

This was measured in KOReader 2026.07.1's own crengine at 1072x1448 and 300 dpi, the Kindle 11's
screen, with synthetic pages that have an 8 px black frame on the edge of the image, so that the
drawn rectangle and the empty space around it can be read off a screenshot. The same crengine
code is in KOReader 2026.07.2-75, which the maintainer's Kindle runs. Drawn size, page 1072x1448:

| KOReader settings | markup of ADR 0013 | with `div { line-height: 0; }` |
|---|---|---|
| default margins and status bar | 1002x1354 | 1011x1366 |
| default margins, "Overlap status bar" | 1023x1382 | 1032x1393 |
| margins 0, status bar off | 1063x1436 (4 and 5 px empty at the sides, 12 below) | **1072x1448, the screen, pixel for pixel** |

A page 965x1448, the shape of a manga page, goes from 902x1354, 921x1382 and 957x1436 to 910x1366,
929x1393 and 965x1448.

The other ways of closing the gap were measured the same way: `font-size: 0` on the wrapper gives
back 9 of the 12 px (1009x1363), and `vertical-align: top`, `bottom` or `middle` on the image give
back none. `display: block`, with or without percentages, gives a different markup that either
stretches the image or changes the structure ADR 0013 chose to keep.

## Decision

- Add one rule to the page's stylesheet: `div { line-height: 0; }`. The page's only `div` is the
  centered block that holds the image (and, for Kindle profiles, upstream's hidden first block).
- Change nothing else in the page: the image keeps its `width` and `height` attributes, stays
  inline, and is never sized in percentages.
- The page's markup was never compared with KCC's by `tools/parity` (it compares pages, package and
  navigation); the unit tests that pin the markup stay as they were, and one is added for the rule.

## Consequences

- In KOReader, a page uses the whole area the reader gives it, and a page that is as big as the
  screen is drawn at 1:1 with the margins at zero, where KOReader copies the picture without
  resampling it. Before, the same page was always drawn smaller than the area and resampled by
  nearest neighbour.
- It does not make the page bigger than the reader's area. The margins and the status bar are the
  reader's settings; with "Overlap status bar" on and the margins at zero, the bar is drawn over
  the bottom of the page.
- It does not change the proportions of the image: both of its sizes keep their ratio.
- The markup differs from KCC 12.0.0's by one stylesheet rule. Only KOReader's engine was tested;
  other readers were not, and the fixed-layout viewport in the `<head>` is unchanged.
