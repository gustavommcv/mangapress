"""The order of file names that `natsort` gives, as KCC 12.0.0 uses it, written down for mangapress's tests.

KCC puts the names of each folder in order with `natsort`'s operating-system ordering. On Linux, without
the optional PyICU library, that is the key made below. This script asks the real library for the order of a
curated list of names and of a seeded set of generated ones, and keeps it in a file that a Rust test reads:

    python tools/parity/natsort_vectors.py            # check that the file is what natsort gives
    python tools/parity/natsort_vectors.py --write    # write it
    python tools/parity/natsort_vectors.py --digits   # print the tables of Unicode digits for digits.rs

Every group in the file is a list of names in the order `natsort` puts them. The locale is the plain "C"
one, where text is compared by code point: other locales order punctuation and accents their own way
(ADR 0019, ORD-5). Names avoid what Python versions treat differently: a dot at the start or the end, or two
dots in a row, and characters Unicode added recently.
"""

import argparse
import locale
from pathlib import Path
import random
import sys
import unicodedata

from natsort import natsort_keygen, ns
from natsort.unicode_numbers import digits_no_decimals

REPO = Path(__file__).resolve().parents[2]
VECTORS = REPO / "crates/mangapress-core/tests/natural_sort_vectors.txt"

HEADER = """\
## Orders of file names from natsort 8.4.0 as KCC 12.0.0 uses it, one group per block.
## Written by tools/parity/natsort_vectors.py; do not edit by hand.
"""

CURATED = [
    # A name that another name continues, and numbers inside a name.
    ["p01.png", "p01 (2).png", "p01-2.png", "p01_b.png"],
    ["cover.png", "cover2.png", "cover_b.png"],
    ["x.png", "x1.png", "x10.png"],
    ["1.png", "1.5.png", "1.10.png", "2.png", "10.png"],
    ["Ch.2.png", "Ch.10.png", "Ch.10.5.png"],
    # Digits of other scripts are numbers.
    ["1.png", "２.png", "3.png", "４.png", "１０.png"],
    ["第１話", "第２話", "第１０話"],
    ["page1.png", "page٢.png", "page३.png", "page10.png"],
    # Characters that stand for a digit are numbers, one at a time.
    ["x1.png", "x².png", "x3.png", "x①.png"],
    ["a1.png", "a1².png", "a１²３.png"],
    # What is and is not an extension.
    ["a.jpg", "a.png", "a1.jpg"],
    ["a.PNG", "b.png", "B.JPG"],
    ["Vol. 1", "Vol. 2", "Vol. 10", "Vol 11"],
    ["name.tar.gz", "name.tar.xz", "name2.tar.gz"],
    ["my.5.png", "my.50.png", "my.6.png"],
    # Case is ignored.
    ["Chapter 1", "chapter 2", "Chapter 10"],
    ["A.jpg", "b.jpg", "C.jpg"],
]

WORDS = ["p", "P", "page", "Page", "cover", "x", "Ch", "ch", "vol", "a", "B", "img", "第", "é"]
NUMBERS = ["1", "01", "001", "2", "10", "11", "9", "100", "１", "２", "１０", "٣", "١٠", "०१", "²", "①", "⑩", "𝟓"]
SEPARATORS = ["", " ", "-", "_", ".", " (", ")", "+", "#", "~", ","]
EXTENSIONS = [".png", ".PNG", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".JPG", ".tar.gz", ".5", ".png.bak", ""]


def key():
    locale.setlocale(locale.LC_COLLATE, "C")
    return natsort_keygen(alg=ns.LOCALE | ns.PATH | ns.IGNORECASE)


def acceptable(name):
    return bool(name) and name[0].isalnum() and name[-1] not in ". " and ".." not in name


def generated_names(rng):
    names = []
    while len(names) < 40:
        pieces = []
        for _ in range(rng.randint(1, 4)):
            pieces.append(rng.choice(WORDS if rng.random() < 0.5 else NUMBERS))
            pieces.append(rng.choice(SEPARATORS))
        name = "".join(pieces).rstrip(" ") + rng.choice(EXTENSIONS)
        if acceptable(name) and name not in names:
            names.append(name)
    return names


def groups():
    order = key()
    rng = random.Random(2026_10_09)
    for names in CURATED + [generated_names(rng) for _ in range(40)]:
        seen, kept = set(), []
        for name in names:
            if order(name) not in seen:
                seen.add(order(name))
                kept.append(name)
        yield sorted(kept, key=order)


def render():
    return HEADER + "".join("---\n" + "\n".join(group) + "\n" for group in groups())


def digit_tables():
    zeros = [cp for cp in range(0x110000) if unicodedata.category(chr(cp)) == "Nd" and unicodedata.digit(chr(cp)) == 0]
    ranges = []
    for cp, value in sorted((ord(c), unicodedata.digit(c)) for c in digits_no_decimals):
        if ranges and ranges[-1][1] == cp - 1 and ranges[-1][2] + (cp - ranges[-1][0]) == value:
            ranges[-1][1] = cp
        else:
            ranges.append([cp, cp, value])
    return zeros, ranges


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--write", action="store_true", help="write the file instead of checking it")
    parser.add_argument("--digits", action="store_true", help="print the digit tables for digits.rs")
    args = parser.parse_args()
    if args.digits:
        zeros, ranges = digit_tables()
        print(f"unicode {unicodedata.unidata_version}\nDECIMAL_ZEROS ({len(zeros)}):", ", ".join(hex(z) for z in zeros))
        print(f"OTHER_DIGITS ({len(ranges)}):", ", ".join(f"({a:#x}, {b:#x}, {v})" for a, b, v in ranges))
        return
    text = render()
    if args.write:
        VECTORS.write_text(text, encoding="utf-8")
        print(f"wrote {VECTORS}")
    elif not VECTORS.exists() or VECTORS.read_text(encoding="utf-8") != text:
        sys.exit(f"{VECTORS} is not what natsort gives; run this script with --write and review the change")
    else:
        print("natural_sort_vectors.txt is what natsort gives")


if __name__ == "__main__":
    main()
