"""Checks mangapress against upstream KCC, page by page.

Runs KCC's own code (from a checkout you provide) and mangapress's real
pipeline over the same pages, under a matrix of options, and fails if they
disagree on anything that isn't codec or resampler noise:

  - the page's detected background;
  - how many pages a source page becomes, in which order, and what each is
    (ordinary page, first/second half of a spread, rotated spread);
  - each output page's size, and whether it is grayscale or colour;
  - the pixels, within a small mean difference;
  - and, where both sides only move pixels around, the pixels exactly: the
    palette dither given the same input, and webtoon strips cut into pages.

    python tools/parity/parity.py --kcc /path/to/kcc            # synthetic corpus
    python tools/parity/parity.py --kcc /path/to/kcc --pages DIR  # plus your own pages

See README.md next to this file.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

import numpy as np
from PIL import Image

import make_corpus
from kcc_oracle import load_kcc

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))

# The KCC release mangapress follows. Another version still runs, with a
# warning: differences may then be KCC's own between releases.
REFERENCE_KCC = "12.0.0"

# Mean absolute difference allowed between the two tools' pixels, in levels
# out of 255. Both sides are taken before the JPEG step (see README.md), so
# this is the processing, with the independent resampler; a real
# divergence — a different crop, a contrast stretch applied by one side only
# — shows up as several levels.
GRAY_LIMIT = 1.0
COLOUR_LIMIT = 1.5

DEFAULT_PROFILE = "K11"
KCC_BASE = ["-p", "{profile}", "-m", "-u", "-f", "EPUB"]
DUMP_BASE = ["--manga", "--upscale"]
SPLIT_BOTH = (["-r", "2"], ["--splitter=both"])

# name, corpus list, extra KCC arguments, extra parity_dump flags, and
# optionally replacement base arguments for either side.
SCENARIOS = [
    ("default options", "pages", SPLIT_BOTH[0], SPLIT_BOTH[1]),
    ("spreads: split", "spreads", ["-r", "0"], ["--splitter=split"]),
    ("spreads: rotate", "spreads", ["-r", "1"], ["--splitter=rotate"]),
    ("spreads: split and rotate", "spreads", *SPLIT_BOTH),
    ("spreads: rotated copy first", "spreads", ["-r", "2", "--rotatefirst"], ["--splitter=both", "--rotatefirst"]),
    # On a Kobo profile of the same resolution: for a Kindle's EPUB, KCC caps
    # an upright spread at 1920px, which mangapress deliberately does not.
    ("spreads: not rotated", "spreads", ["-r", "2", "--norotate"], ["--splitter=both", "--norotate"], ["-p", "KoC", "-m", "-u", "-f", "EPUB"]),
    ("spreads: rotated clockwise", "spreads", ["-r", "2", "--rotateright"], ["--splitter=both", "--rotateright"]),
    ("strips restacked 2x2", "spreads", ["-r", "2", "--maximizestrips"], ["--splitter=both", "--maximizestrips"]),
    ("crop: margins only", "pages", ["-r", "2", "-c", "1"], ["--splitter=both", "--crop=margins"]),
    ("crop: none", "pages", ["-r", "2", "-c", "0"], ["--splitter=both", "--crop=disabled"]),
    ("crop: power 2", "pages", ["-r", "2", "--cp", "2.0"], ["--splitter=both", "--croppingpower=2.0"]),
    # KCC takes the minimum as a ratio, mangapress as a percentage.
    ("crop: only when 90% of the page is kept", "pages", ["-r", "2", "--cm", "0.9"], ["--splitter=both", "--croppingminimum=90"]),
    ("crop: 5% of the margin preserved", "pages", ["-r", "2", "--preservemargin", "5"], ["--splitter=both", "--preservemargin=5"]),
    ("crop: between panels, rows", "pages", ["-r", "2", "--ipc", "1"], ["--splitter=both", "--ipc=horizontal"]),
    ("crop: between panels, rows and columns", "pages", ["-r", "2", "--ipc", "2"], ["--splitter=both", "--ipc=both"]),
    ("autolevel", "pages", ["-r", "2", "--autolevel"], ["--splitter=both", "--autolevel"]),
    ("no autocontrast", "pages", ["-r", "2", "--noautocontrast"], ["--splitter=both", "--noautocontrast"]),
    ("gamma 1.8", "pages", ["-r", "2", "-g", "1.8"], ["--splitter=both", "--gamma=1.8"]),
    ("stretch", "pages", ["-r", "2", "-s"], ["--splitter=both", "--stretch"]),
    ("wallpaper: cropped to fill the screen", "pages", ["-r", "2", "--wallpaper"], ["--splitter=both", "--wallpaper"]),
    ("wallpaper, spreads", "spreads", ["-r", "2", "--wallpaper"], ["--splitter=both", "--wallpaper"]),
    ("rainbow eraser", "pages", ["-r", "2", "--eraserainbow"], ["--splitter=both", "--eraserainbow"]),
    ("no upscale", "pages", ["-r", "2"], ["--splitter=both"], ["-p", "{profile}", "-m", "-f", "EPUB"], ["--manga"]),
    ("CBZ: padded to the screen", "spreads", ["-r", "2"], ["--splitter=both", "--format=cbz"], ["-p", "{profile}", "-m", "-u", "-f", "CBZ"]),
    ("CBZ: black borders", "pages", ["-r", "2", "--blackborders"], ["--splitter=both", "--format=cbz", "--blackborders"], ["-p", "{profile}", "-m", "-u", "-f", "CBZ"]),
    ("CBZ: white borders", "pages", ["-r", "2", "--whiteborders"], ["--splitter=both", "--format=cbz", "--whiteborders"], ["-p", "{profile}", "-m", "-u", "-f", "CBZ"]),
    ("KDX: custom width disables the CBZ height override", "spreads", ["-r", "2", "--customwidth", "824"],
     ["--splitter=both", "--format=cbz", "--customwidth=824"], ["-p", "KDX", "-m", "-u", "-f", "CBZ"]),
    ("KDX: custom height disables the CBZ height override", "spreads", ["-r", "2", "--customheight", "1000"],
     ["--splitter=both", "--format=cbz", "--customheight=1000"], ["-p", "KDX", "-m", "-u", "-f", "CBZ"]),
    ("KS3: custom width disables the EPUB width cap", "spreads", ["-r", "2", "--customwidth", "1986"],
     ["--splitter=both", "--customwidth=1986"], ["-p", "KS3", "-m", "-u", "-f", "EPUB"]),
    ("KS3: custom height disables the EPUB width cap", "spreads", ["-r", "2", "--customheight", "2648"],
     ["--splitter=both", "--customheight=2648"], ["-p", "KS3", "-m", "-u", "-f", "EPUB"]),
    ("colour pages, grayscale output", "colour", *SPLIT_BOTH),
    ("colour pages, grayscale output, colour autocontrast", "colour", ["-r", "2", "--colorautocontrast"], ["--splitter=both", "--colorautocontrast"]),
    ("colour output", "colour", ["-r", "2", "--forcecolor"], ["--splitter=both", "--forcecolor"]),
    ("colour output, autocontrast and autolevel", "colour", ["-r", "2", "--forcecolor", "--colorautocontrast", "--autolevel"],
     ["--splitter=both", "--forcecolor", "--colorautocontrast", "--autolevel"]),
    ("colour output, rainbow eraser", "colour", ["-r", "2", "--forcecolor", "--eraserainbow"], ["--splitter=both", "--forcecolor", "--eraserainbow"]),
    ("colour output, gamma 1.8", "colour", ["-r", "2", "--forcecolor", "-g", "1.8"], ["--splitter=both", "--forcecolor", "--gamma=1.8"]),
    ("OTHER: odd custom dimensions, transparency and LTR", "edges", ["-c", "0", "--forcepng"],
     ["--crop=disabled", "--forcepng", "--customwidth=127", "--customheight=193"],
     ["-p", "OTHER", "-u", "-f", "EPUB", "--customwidth", "127", "--customheight", "193"], ["--upscale"]),
    ("OTHER: crop cap boundaries", "crop_edges", ["-c", "1"],
     ["--crop=margins", "--customwidth=128", "--customheight=192"],
     ["-p", "OTHER", "-f", "EPUB", "--customwidth", "128", "--customheight", "192"], []),
]

EXTENDED_SCENARIOS = [
    ("OTHER: odd custom dimensions, transparency and RTL", "edges", ["-m", "-c", "0", "--forcepng"],
     ["--manga", "--crop=disabled", "--forcepng", "--customwidth=127", "--customheight=193"],
     ["-p", "OTHER", "-u", "-f", "EPUB", "--customwidth", "127", "--customheight", "193"], ["--upscale"]),
    ("OTHER: odd dimensions, color PNG and rotated copy first", "edges", ["-c", "0", "-r", "2", "--rotatefirst", "--forcepng", "--forcecolor", "--force-png-rgb"],
     ["--crop=disabled", "--splitter=both", "--rotatefirst", "--forcepng", "--forcecolor", "--force-png-rgb", "--customwidth=127", "--customheight=193"],
     ["-p", "OTHER", "-u", "-f", "EPUB", "--customwidth", "127", "--customheight", "193"], ["--upscale"]),
    ("OTHER: crop minimum at 81%", "crop_edges", ["-c", "1", "--cm", "0.81"],
     ["--crop=margins", "--croppingminimum=81", "--customwidth=128", "--customheight=192"],
     ["-p", "OTHER", "-f", "EPUB", "--customwidth", "128", "--customheight", "192"], []),
]

# A small cross-device check of geometry and grayscale/color processing.
# The full K11 matrix and exact dither/webtoon checks remain the baseline.
SMOKE_SCENARIOS = {"default options", "CBZ: padded to the screen", "colour output"}

ROLES = {"N": "Normal", "S1": "SplitFirst", "S2": "SplitSecond", "R": "Rotated"}

# Device palettes, for the dither check: the gray levels each profile's
# screen shows.
PALETTES = {
    "K11": [level * 17 for level in range(16)],
    "K2": [level * 17 for level in range(14)] + [255],
    "K1": [0, 85, 170, 255],
}


class Report:
    def __init__(self):
        self.failures = []
        self.checked = 0

    def fail(self, where, what):
        self.failures.append(f"{where}: {what}")

    def ok(self):
        self.checked += 1


def run(command, **kwargs):
    result = subprocess.run(command, capture_output=True, text=True, **kwargs)
    if result.returncode != 0:
        raise SystemExit(f"command failed: {' '.join(command)}\n{result.stdout}\n{result.stderr}")
    return result.stdout


def matches(name, filters):
    return not filters or any(text in name for text in filters)


def select_scenarios(filters=None, smoke=False, extended=False):
    return [scenario for scenario in SCENARIOS + (EXTENDED_SCENARIOS if extended else [])
            if (scenario[0] in SMOKE_SCENARIOS if smoke else matches(scenario[0], filters))]


def scenario_bases(profile, bases):
    kcc_base = bases[0] if bases else KCC_BASE
    dump_base = bases[1] if len(bases) > 1 else DUMP_BASE
    return [arg.format(profile=profile) for arg in kcc_base], list(dump_base)


def manifest_pages(directory, name):
    with open(os.path.join(directory, name), encoding="utf-8") as handle:
        return json.load(handle)["pages"]


def dither_inputs(directory):
    paths = [os.path.join(directory, piece["file"])
             for page in manifest_pages(directory, "kcc.json") for piece in page["pieces"]]
    if not paths:
        raise SystemExit("dither comparison has no reference pages")
    return paths


def comparison_image(path):
    with Image.open(path) as image:
        # A grayscale palette can decode as RGB. Check its actual channels
        # rather than rejecting a lossless encoding of the same gray pixels.
        rgb = image.convert("RGB")
        values = np.asarray(rgb)
        gray = np.array_equal(values[:, :, 0], values[:, :, 1]) and np.array_equal(values[:, :, 1], values[:, :, 2])
        return rgb.convert("L") if gray else rgb


def compare_pages(report, name, files, kcc_dir, dump_dir):
    kcc = manifest_pages(kcc_dir, "kcc.json")
    ours = manifest_pages(dump_dir, "mangapress.json")
    if not files or len(files) != len(kcc) or len(files) != len(ours):
        report.fail(name, f"source page count: input {len(files)}, KCC {len(kcc)}, mangapress {len(ours)}")
        return 0.0
    worst = 0.0
    for path, theirs, mine in zip(files, kcc, ours):
        where = f"{name} / {os.path.basename(path)}"
        if theirs["background"] != mine["background"]:
            report.fail(where, f"background KCC {theirs['background']}, mangapress {mine['background']}")
            continue
        their_roles = [ROLES[piece["mode"]] for piece in theirs["pieces"]]
        my_roles = [piece["role"] for piece in mine["pieces"]]
        if their_roles != my_roles:
            report.fail(where, f"pages produced: KCC {their_roles}, mangapress {my_roles}")
            continue
        if not their_roles:
            report.fail(where, "pages produced: neither tool returned an output page")
            continue
        for a, b in zip(theirs["pieces"], mine["pieces"]):
            if a["size"] != b["size"]:
                report.fail(where, f"{ROLES[a['mode']]} size KCC {a['size']}, mangapress {b['size']}")
                continue
            if a["black_background"] != b["black_background"]:
                report.fail(where, f"black page background KCC {a['black_background']}, mangapress {b['black_background']}")
                continue
            x = comparison_image(os.path.join(kcc_dir, a["file"]))
            y = comparison_image(os.path.join(dump_dir, b["file"]))
            if list(x.size) != a["size"] or list(y.size) != b["size"]:
                report.fail(where, "encoded image dimensions disagree with the manifest")
                continue
            if x.mode != y.mode:
                report.fail(where, f"KCC wrote {x.mode}, mangapress {y.mode}")
                continue
            difference = np.abs(np.asarray(x, dtype=np.int16) - np.asarray(y, dtype=np.int16)).mean()
            worst = max(worst, difference)
            limit = COLOUR_LIMIT if x.mode == "RGB" else GRAY_LIMIT
            if difference > limit:
                report.fail(where, f"{ROLES[a['mode']]} pixels differ by {difference:.2f} levels on average (limit {limit})")
            else:
                report.ok()
    return worst


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--kcc", required=True, help="path to a KCC checkout (the directory containing kindlecomicconverter/)")
    parser.add_argument("--pages", help="a folder of your own pages to add to the default scenario (they stay local)")
    parser.add_argument("--work", default=os.path.join(REPO, "target", "parity"), help="where to write everything")
    parser.add_argument("--profile", default=DEFAULT_PROFILE, help="device profile code (default: K11)")
    parser.add_argument("--extended", action="store_true", help="include pre-release boundary/option interactions")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--only", action="append", help="run scenarios containing this text; repeat to select more")
    selection.add_argument("--smoke", action="store_true", help="run only default, CBZ padding, and color-output scenarios")
    args = parser.parse_args()

    kcc = os.path.abspath(args.kcc)
    if not os.path.isfile(os.path.join(kcc, "kindlecomicconverter", "image.py")):
        raise SystemExit(f"{kcc} doesn't look like a KCC checkout")
    version, kcc_cli, _, kcc_image = load_kcc(kcc)
    profile_data = kcc_image.ProfileData.Profiles.get(args.profile)
    if not profile_data or not all(profile_data[1]):
        parser.error(f"{args.profile!r} is not a device profile with a built-in resolution")
    selected = select_scenarios(args.only, args.smoke, args.extended)
    check_dither = not args.smoke and matches("dither", args.only)
    check_webtoon = not args.smoke and matches("webtoon", args.only)
    if not selected and not check_dither and not check_webtoon:
        parser.error("no scenarios match --only")
    work = os.path.abspath(args.work)
    os.makedirs(work, exist_ok=True)
    oracle = [sys.executable, os.path.join(HERE, "kcc_oracle.py"), kcc]

    print("building parity_dump ...")
    run(["cargo", "build", "--release", "--locked", "-p", "mangapress-core", "--example", "parity_dump"], cwd=REPO)
    dump = os.path.join(REPO, "target", "release", "examples", "parity_dump" + (".exe" if os.name == "nt" else ""))

    corpus = make_corpus.write(os.path.join(work, "corpus"))
    scenarios = list(selected)
    if args.pages:
        extensions = (".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp")
        corpus["yours"] = sorted(os.path.join(root, name) for root, _, names in os.walk(os.path.abspath(args.pages))
                                 for name in names if name.lower().endswith(extensions))
        extras = [("your pages, default options", "yours", *SPLIT_BOTH),
                  ("your pages, colour output", "yours", ["-r", "2", "--forcecolor"], ["--splitter=both", "--forcecolor"])]
        scenarios.extend(scenario for scenario in extras if args.smoke or matches(scenario[0], args.only))
        print(f"{len(corpus['yours'])} of your own pages added")

    report = Report()
    dither_source = None
    print(f"comparing profile {args.profile} against KCC {version}")
    for index, (name, key, kcc_extra, dump_extra, *bases) in enumerate(scenarios):
        files = corpus[key]
        list_file = os.path.join(work, f"list_{key}.txt")
        Path(list_file).write_text("\n".join(files) + "\n", encoding="utf-8")
        kcc_dir, dump_dir = os.path.join(work, f"{index:02d}_kcc"), os.path.join(work, f"{index:02d}_mangapress")
        kcc_base, dump_base = scenario_bases(args.profile, bases)
        run(oracle + [kcc_dir, list_file] + kcc_base + kcc_extra)
        if name == "default options":
            dither_source = kcc_dir
        # KCC's side is its pixels before its JPEG save; ours is lossless too unless the scenario is about PNG output.
        lossless = [] if any(flag.startswith("--forcepng") for flag in dump_base + dump_extra) else ["--lossless"]
        run([dump, dump_dir, list_file, kcc_base[kcc_base.index("-p") + 1]] + dump_base + dump_extra + lossless)
        before = len(report.failures)
        worst = compare_pages(report, name, files, kcc_dir, dump_dir)
        status = "ok  " if len(report.failures) == before else "FAIL"
        comparison_profile = kcc_base[kcc_base.index("-p") + 1]
        print(f"  {status} [{comparison_profile}] {name:52s} {len(files):3d} pages, largest mean difference {worst:.2f}")

    if check_dither:
        # The same grayscale page into both quantizers: every pixel must land
        # on the same level. Upstream's is one Pillow call.
        if dither_source is None:
            dither_source = os.path.join(work, "dither_source_kcc")
            source_list = os.path.join(work, "list_dither_source.txt")
            with open(source_list, "w", encoding="utf-8") as handle:
                handle.write("\n".join(corpus["pages"]) + "\n")
            kcc_base, _ = scenario_bases(args.profile, [])
            run(oracle + [dither_source, source_list] + kcc_base + SPLIT_BOTH[0])
        grays = dither_inputs(dither_source)
        list_file = os.path.join(work, "list_gray.txt")
        Path(list_file).write_text("\n".join(grays) + "\n", encoding="utf-8")
        for profile, levels in PALETTES.items():
            out_dir = os.path.join(work, f"dither_{profile}")
            run([dump, out_dir, list_file, profile, "--quantize-only"])
            palette = Image.new("P", (1, 1))
            palette.putpalette([value for level in levels for value in (level, level, level)])
            differing = 0
            for number, path in enumerate(grays):
                theirs = np.asarray(Image.open(path).convert("L").convert("RGB").quantize(palette=palette).convert("L"))
                mine = np.asarray(Image.open(os.path.join(out_dir, f"{number:04d}.png")).convert("L"))
                differing += int((theirs != mine).sum())
            if differing:
                report.fail(f"dither, {len(levels)} levels", f"{differing} pixels differ from Pillow's")
            else:
                report.ok()
            print(f"  {'ok  ' if not differing else 'FAIL'} dither to {len(levels):2d} levels, same input{'':24s} {len(grays):3d} pages, {differing} pixels differ")

    if check_webtoon:
        # The table is not always the processing target (e.g. Scribe EPUB).
        webtoon_options = kcc_cli.checkOptions(kcc_cli.makeParser().parse_args(
            ["-p", args.profile, "-w", "-f", "EPUB", "unused-input"]))
        width, height = webtoon_options.profileData[1]
        for name, chapter in corpus["webtoon"].items():
            kcc_dir, dump_dir = os.path.join(work, f"{name}_kcc"), os.path.join(work, f"{name}_mangapress")
            run(oracle + [kcc_dir, "--webtoon-dir", chapter, str(width), str(height)])
            for stale in (os.listdir(dump_dir) if os.path.isdir(dump_dir) else []):
                os.unlink(os.path.join(dump_dir, stale))
            run([dump, dump_dir, chapter, args.profile, "--webtoon-dir"])
            theirs = sorted(os.listdir(kcc_dir))
            mine = sorted(page for page in os.listdir(dump_dir) if page.startswith("page-"))
            same_image = lambda a, b: np.array_equal(np.asarray(Image.open(a).convert("RGB")), np.asarray(Image.open(b).convert("RGB")))
            problem = None
            if not same_image(kcc_dir + "_strip.png", os.path.join(dump_dir, "strip.png")):
                problem = "merged strips differ"
            elif len(theirs) != len(mine):
                problem = f"KCC cut {len(theirs)} pages, mangapress {len(mine)}"
            elif not all(same_image(os.path.join(kcc_dir, a), os.path.join(dump_dir, b)) for a, b in zip(theirs, mine)):
                problem = "cut pages differ"
            if problem:
                report.fail(f"webtoon / {name}", problem)
            else:
                report.ok()
            print(f"  {'ok  ' if not problem else 'FAIL'} webtoon strips cut into pages: {name:20s} {len(theirs):3d} pages, {'identical' if not problem else problem}")

            # ...and those pages through the rest of webtoon mode.
            pages = [os.path.join(kcc_dir, page) for page in theirs]
            list_file = os.path.join(work, f"list_{name}.txt")
            Path(list_file).write_text("\n".join(pages) + "\n", encoding="utf-8")
            run(oracle + [kcc_dir + "_pages", list_file, "-p", args.profile, "-w", "-f", "EPUB"])
            run([dump, dump_dir + "_pages", list_file, args.profile, "--webtoon"])
            before = len(report.failures)
            worst = compare_pages(report, f"webtoon pages / {name}", pages, kcc_dir + "_pages", dump_dir + "_pages")
            print(f"  {'ok  ' if len(report.failures) == before else 'FAIL'} webtoon pages processed: {name:26s} {len(pages):3d} pages, largest mean difference {worst:.2f}")

    print()
    if version and version != REFERENCE_KCC:
        print(f"note: this checkout is KCC {version}; mangapress follows {REFERENCE_KCC}, so a difference may be KCC's own between the two")
    if not report.checked and not report.failures:
        report.fail("comparison", "no output pages were checked")
    if report.failures:
        print(f"{len(report.failures)} difference(s) from KCC {version}:")
        for failure in report.failures:
            print("  -", failure)
        raise SystemExit(1)
    print(f"mangapress matches KCC {version}: {report.checked} checks passed")


if __name__ == "__main__":
    main()
