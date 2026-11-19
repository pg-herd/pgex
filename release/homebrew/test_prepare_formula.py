import unittest
from pathlib import Path

from prepare_formula import prepare_formula


class PrepareFormulaTests(unittest.TestCase):
    def setUp(self):
        self.formula = (Path(__file__).resolve().parents[2] / "Formula/pgex.rb").read_text()
        self.checksum = "a" * 64

    def test_new_release_replaces_source_and_removes_old_bottle(self):
        result = prepare_formula(self.formula, "v12.34.56", self.checksum)
        self.assertIn('/v12.34.56/pgex-v12.34.56-aarch64-apple-darwin.tar.gz"', result)
        self.assertIn('  version "12.34.56"', result)
        self.assertIn(f'  sha256 "{self.checksum}"', result)
        self.assertNotIn("bottle do", result)
        self.assertEqual(result[result.index("  depends_on"):], self.formula[self.formula.index("  depends_on"):])

    def test_preparing_an_unbottled_formula_is_repeatable(self):
        result = prepare_formula(self.formula, "v12.34.56", self.checksum)
        self.assertEqual(prepare_formula(result, "v12.34.56", self.checksum), result)

    def test_rejects_invalid_release_inputs(self):
        for tag, checksum in [("main", self.checksum), ("v1.0.0-rc1", self.checksum), ("v1.0.0", "not-a-sha256")]:
            with self.subTest(tag=tag, checksum=checksum), self.assertRaises(ValueError):
                prepare_formula(self.formula, tag, checksum)

    def test_rejects_missing_or_duplicate_formula_fields(self):
        for formula in [self.formula.replace('  version ', '  # version '), self.formula + '\n  version "2.0.0"\n']:
            with self.subTest(formula=formula), self.assertRaises(ValueError):
                prepare_formula(formula, "v1.0.0", self.checksum)


if __name__ == "__main__":
    unittest.main()
