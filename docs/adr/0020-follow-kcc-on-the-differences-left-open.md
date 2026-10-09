# 20. Follow KCC on the differences that ADR 0019 left open

Date: 2026-10-09

## Status

Accepted on 2026-10-09. The maintainer decided it, and approves it by merging the pull request
that carries it.

## Context

[ADR 0019](0019-differences-from-kcc-found-by-the-october-comparison.md) kept nine differences
from KCC 12.0.0 on purpose and left eight open (ORD-1, ORD-2, FILE-3, META-6, JPEG-1, CUST-1,
CUST-2, SIZE-1), most of them because they looked small: rare in real books, or without effect
on the device its author reads on. The maintainer read the list and decided otherwise. They are
real differences from the tool mangapress is meant to match (ADR 0013), KCC probably does each
of them because a book of that size needed it, and a user meets one without being able to tell
why. The goal that mangapress behaves like the named KCC release does not make an exception for
small differences.

## Decision

mangapress follows KCC 12.0.0 on all eight, and on the file size that goes with JPEG-1 (the
JPEG-2 row of ADR 0019, which stays as a difference in rounding only). A row leaves the
"known and left open" table of ADR 0019 when its case moves from `tools/parity/differences.py`
into the routine comparison, so that `differences.py` lists exactly what is still open and is
empty when the work is done.

How each is followed, where the way was a decision:

- **META-6, CUST-1, CUST-2.** The padding of the volume and issue numbers puts zeros after a
  sign, as a number is written. A custom size is a size given as a non-zero width or height: it
  makes the device an ordinary one in two respects, sixteen gray levels, and the default JPEG
  quality of an ordinary device. A quality given on the command line wins, as before.
- **FILE-3.** A damaged image is read as far as it goes and the rest of the page is blank, as
  KCC does. What ADR 0019 gave as the reason to stop, that stopping names the damaged file, is
  kept by saying so: the run reports the file in a warning, and the book is made.
- **ORD-1, ORD-2.** A name is split from its extensions and compared without them first, and
  digits of every script count as numbers, which is what KCC's sorting library does. Its order
  also depends on the platform and the locale; mangapress takes the one answer that does not
  (ADR 0019, ORD-5). The order is checked against the real library, name by name, and not only
  on a few books. Mangabind orders the pages of a chapter by its own rules and has the same two
  differences, so it is changed too: the book Mangabound makes has the order KCC would give once
  both are released.
- **JPEG-1, SIZE-1.** The encoders are chosen so that a color JPEG keeps its chroma at half size
  in both directions and a PNG is as small as KCC's. A new dependency, if one is needed, gets its
  own record.

## Consequences

The tool and KCC give the same pages in the same order for every input the comparison covers,
with the limits that stay: the bytes of a JPEG (two encoders round differently), the
platform-dependent parts of KCC's ordering, which mangapress fixes to one answer, and the
differences ADR 0019 keeps on purpose.

Each fix is its own pull request with the case that proves it. Releases and the pins of
Mangabound and Mangabind are not part of this decision.
