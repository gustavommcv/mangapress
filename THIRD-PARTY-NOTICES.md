# Third-party notices

mangapress itself is licensed under MIT or Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)). It is an independent project: not affiliated with, and not
endorsed by, KCC, Pillow, or their authors.

Its code is its own, written in Rust for this project. But it sets out to produce the same pages
as another program, and two projects' work shows in it closely enough that their notices belong
here.

## Compiled dependencies

Release archives also include `DEPENDENCY-LICENSES.txt`: original license texts and copyright
notices for the locked Rust dependency graph, including bundled native-library notices gathered
from those packages. It is generated separately for each release target. Build dependencies are
included as a conservative superset; development-only dependencies are excluded. See
[notice generation](tools/licenses/README.md) for the tooling and source-file clarifications.

## Independent JPEG Group

This software is based in part on the work of the Independent JPEG Group.

The JPEG pages and covers mangapress writes are encoded with the
[`jpeg-encoder`](https://github.com/vstroebel/jpeg-encoder) crate, whose forward DCT is a port
of the integer DCT of libjpeg (through mozjpeg). The IJG's license text, which the crate
carries in the header of that file, is reproduced in `DEPENDENCY-LICENSES.txt` with the
crate's MIT and Apache-2.0 licenses.

## KCC (Kindle Comic Converter)

<https://github.com/ciromattia/kcc>

mangapress reproduces the behavior of KCC's conversion pipeline: given the same pages and options,
the same book. It does so by reimplementing what KCC does, not by including KCC's code — see
[docs/adr/0007-gplv3-boundary-kcc-image-rs.md](docs/adr/0007-gplv3-boundary-kcc-image-rs.md) and
[docs/adr/0013-follow-a-named-kcc-release.md](docs/adr/0013-follow-a-named-kcc-release.md).

What follows KCC's own text closely is confined to things a compatible tool has to share with it:
the markup of the EPUB package (the same elements and metadata, so that readers treat the book the
same way), the names of command-line options, and the wording of a few option descriptions. These
come from files under KCC's ISC license, whose notice is:

```
ISC LICENSE

Copyright (c) 2012-2025 Ciro Mattia Gonano <ciromattia@gmail.com>
Copyright (c) 2013-2019 Paweł Jastrzębski <pawelj@iosphe.re>
Copyright (c) 2021-2023 Darodi (https://github.com/darodi)
Copyright (c) 2023-2025 Alex Xu (https://github.com/axu2)

Permission to use, copy, modify, and/or distribute this software for
any purpose with or without fee is hereby granted, provided that the
above copyright notice and this permission notice appear in all
copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL
WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL
DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA
OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```

Two of KCC's files, `image.py` and `dualmetafix.py`, are under the GNU General Public License,
version 3 or later, instead. mangapress contains no code and no comments from them. What
`image.py` does is reimplemented from its behavior, and the device table carries the same facts
about each device (its name, screen resolution and gray levels).

## Pillow

<https://github.com/python-pillow/Pillow>

KCC does its image work through Pillow, so "the same pages as KCC" means Pillow's arithmetic.
mangapress's resampler (`crates/mangapress-core/src/resample.rs`), its palette dither
(`quantize.rs`) and its color conversions (`color.rs`) are written to give Pillow's results bit
for bit, and follow the way Pillow computes them. Pillow's notice is:

```
The Python Imaging Library (PIL) is

    Copyright © 1997-2011 by Secret Labs AB
    Copyright © 1995-2011 by Fredrik Lundh and contributors

Pillow is the friendly PIL fork. It is

    Copyright © 2010 by Jeffrey 'Alex' Clark and contributors

Like PIL, Pillow is licensed under the open source MIT-CMU License:

By obtaining, using, and/or copying this software and/or its associated
documentation, you agree that you have read, understood, and will comply
with the following terms and conditions:

Permission to use, copy, modify and distribute this software and its
documentation for any purpose and without fee is hereby granted,
provided that the above copyright notice appears in all copies, and that
both that copyright notice and this permission notice appear in supporting
documentation, and that the name of Secret Labs AB or the author not be
used in advertising or publicity pertaining to distribution of the software
without specific, written prior permission.

SECRET LABS AB AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
SOFTWARE, INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS.
IN NO EVENT SHALL SECRET LABS AB OR THE AUTHOR BE LIABLE FOR ANY SPECIAL,
INDIRECT OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE
OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```
