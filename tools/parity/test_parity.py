"""Regression tests for comparison bookkeeping, not a substitute for KCC runs."""

import copy
import json
from pathlib import Path
import tempfile
import unittest

from PIL import Image

import parity


class ScenarioTests(unittest.TestCase):
    def test_selected_profile_reaches_default_and_replacement_bases(self):
        for profile in ("K1", "KDX", "KS3", "KoLC", "RmkPPMove"):
            for scenario in parity.SCENARIOS:
                if scenario[0] == "spreads: not rotated" or scenario[0].startswith(("KDX:", "KS3:", "OTHER:")):
                    continue  # Explicitly labelled fixed-profile regressions.
                kcc, _ = parity.scenario_bases(profile, scenario[4:])
                self.assertEqual(kcc[kcc.index("-p") + 1], profile)
        self.assertIn("{profile}", parity.KCC_BASE)

    def test_fixed_regressions_keep_their_named_profile(self):
        for scenario in parity.SCENARIOS:
            if scenario[0].startswith(("KDX:", "KS3:", "OTHER:")):
                kcc, _ = parity.scenario_bases("K1", scenario[4:])
                self.assertEqual(kcc[kcc.index("-p") + 1], scenario[0].split(":")[0])

    def test_smoke_selects_exactly_the_small_geometry_and_color_matrix(self):
        names = {scenario[0] for scenario in parity.select_scenarios(smoke=True)}
        self.assertEqual(names, {"default options", "CBZ: padded to the screen", "colour output"})

    def test_repeatable_filters_select_a_union_without_duplicates(self):
        selected = parity.select_scenarios(["CBZ:", "padded"])
        self.assertEqual(len(selected), 3)
        self.assertTrue(all(scenario[0].startswith("CBZ:") for scenario in selected))
        self.assertEqual(parity.select_scenarios(["not a scenario"]), [])

    def test_extended_cases_are_opt_in_and_do_not_enlarge_smoke(self):
        self.assertGreater(len(parity.select_scenarios(extended=True)), len(parity.select_scenarios()))
        self.assertEqual(parity.select_scenarios(smoke=True, extended=True), parity.select_scenarios(smoke=True))


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.kcc = Path(self.temp.name) / "kcc"
        self.ours = Path(self.temp.name) / "ours"
        self.kcc.mkdir()
        self.ours.mkdir()
        self.theirs = {"background": "white", "pieces": [
            {"mode": "N", "size": [8, 12], "black_background": False, "file": "p.png"}
        ]}
        self.mine = copy.deepcopy(self.theirs)
        self.mine["pieces"][0]["role"] = "Normal"
        for directory in (self.kcc, self.ours):
            Image.new("L", (8, 12), 90).save(directory / "p.png")

    def write_manifests(self, theirs, ours):
        (self.kcc / "kcc.json").write_text(json.dumps({"pages": theirs}), encoding="utf-8")
        (self.ours / "mangapress.json").write_text(json.dumps({"pages": ours}), encoding="utf-8")

    def compare(self, files, theirs, ours):
        self.write_manifests(theirs, ours)
        report = parity.Report()
        parity.compare_pages(report, "fixture", files, str(self.kcc), str(self.ours))
        return report

    def test_missing_source_pages_never_pass_via_zip_truncation(self):
        for theirs, ours in (([], [self.mine]), ([self.theirs], []), ([], [])):
            with self.subTest(kcc=len(theirs), ours=len(ours)):
                report = self.compare(["input.png"], theirs, ours)
                self.assertEqual(report.checked, 0)
                self.assertEqual(len(report.failures), 1)
                self.assertIn("source page count", report.failures[0])

    def test_empty_comparisons_fail(self):
        report = self.compare([], [], [])
        self.assertEqual(report.checked, 0)
        self.assertEqual(len(report.failures), 1)

    def test_identical_pages_are_actually_counted(self):
        report = self.compare(["input.png"], [self.theirs], [self.mine])
        self.assertEqual(report.checked, 1)
        self.assertEqual(report.failures, [])

    def test_wrong_page_geometry_is_a_failure(self):
        self.mine["pieces"][0]["size"] = [8, 10]
        report = self.compare(["input.png"], [self.theirs], [self.mine])
        self.assertEqual(report.checked, 0)
        self.assertIn("size", report.failures[0])

    def test_missing_spread_pieces_are_a_failure(self):
        self.mine["pieces"] = []
        report = self.compare(["input.png"], [self.theirs], [self.mine])
        self.assertEqual(report.checked, 0)
        self.assertIn("pages produced", report.failures[0])

    def test_an_empty_page_pair_cannot_hide_among_successful_pages(self):
        empty = {"background": "white", "pieces": []}
        report = self.compare(["empty.png", "input.png"], [empty, self.theirs], [empty, self.mine])
        self.assertEqual(report.checked, 1)
        self.assertEqual(len(report.failures), 1)
        self.assertIn("neither tool returned", report.failures[0])

    def test_pixel_differences_are_a_failure(self):
        Image.new("L", (8, 12), 0).save(self.ours / "p.png")
        report = self.compare(["input.png"], [self.theirs], [self.mine])
        self.assertEqual(report.checked, 0)
        self.assertIn("pixels differ", report.failures[0])

    def test_declared_dimensions_cannot_hide_a_wrong_encoded_image(self):
        Image.new("L", (8, 10), 90).save(self.ours / "p.png")
        self.assertIn("encoded image dimensions", self.compare(["input"], [self.theirs], [self.mine]).failures[0])

    def test_gray_rgb_container_matches_but_an_actual_color_pixel_does_not(self):
        image = Image.new("RGB", (8, 12), (90, 90, 90))
        image.save(self.ours / "p.png")
        self.assertEqual(self.compare(["input"], [self.theirs], [self.mine]).checked, 1)
        image.putpixel((0, 0), (90, 90, 91))
        image.save(self.ours / "p.png")
        self.assertIn("wrote", self.compare(["input"], [self.theirs], [self.mine]).failures[0])

    def test_dither_uses_the_manifest_and_not_stale_files(self):
        self.write_manifests([self.theirs], [self.mine])
        Image.new("L", (1, 1)).save(self.kcc / "stale.png")
        self.assertEqual(parity.dither_inputs(str(self.kcc)), [str(self.kcc / "p.png")])

    def test_empty_dither_input_is_not_three_successful_palette_checks(self):
        self.write_manifests([], [])
        with self.assertRaisesRegex(SystemExit, "no reference pages"):
            parity.dither_inputs(str(self.kcc))


if __name__ == "__main__":
    unittest.main()
